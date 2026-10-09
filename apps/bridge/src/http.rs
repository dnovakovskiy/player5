//! Minimal HTTP/1.1 for the bridge: read one request head with size and
//! time limits, parse it, and answer with a WebSocket upgrade, a small JSON
//! document, or a static file. One request per connection.

use std::io::{self, Read, Write};
use std::net::TcpStream;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, Instant};

use crate::ws;

/// Largest request head accepted.
pub const MAX_HEAD: usize = 16 * 1024;
/// Time allowed to send the whole request head.
pub const HEAD_DEADLINE: Duration = Duration::from_secs(5);

/// A parsed request head.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Request {
    /// Method, e.g. `GET`.
    pub method: String,
    /// Path without the query string, still percent-encoded.
    pub path: String,
    /// Headers in order, names lowercased.
    pub headers: Vec<(String, String)>,
}

impl Request {
    /// First header with this (lowercase) name.
    #[must_use]
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .iter()
            .find(|(n, _)| n == name)
            .map(|(_, v)| v.as_str())
    }

    /// Whether a comma-separated header contains `token` (case-insensitive).
    #[must_use]
    pub fn header_has_token(&self, name: &str, token: &str) -> bool {
        self.headers
            .iter()
            .filter(|(n, _)| n == name)
            .flat_map(|(_, v)| v.split(','))
            .any(|t| t.trim().eq_ignore_ascii_case(token))
    }
}

/// Why a request could not be read.
#[derive(Debug)]
pub enum ReadError {
    /// Socket error or the client went away.
    Io(io::Error),
    /// Head larger than [`MAX_HEAD`].
    TooLarge,
    /// Not sent within [`HEAD_DEADLINE`].
    Timeout,
    /// Unparseable.
    Malformed,
}

/// Reads one request head. Returns it plus any bytes read past the head.
pub fn read_request(stream: &mut TcpStream) -> Result<(Request, Vec<u8>), ReadError> {
    let start = Instant::now();
    let mut buf = Vec::with_capacity(1024);
    let mut chunk = [0u8; 2048];
    loop {
        if let Some(end) = find_head_end(&buf) {
            let rest = buf[end + 4..].to_vec();
            let head = std::str::from_utf8(&buf[..end]).map_err(|_| ReadError::Malformed)?;
            return parse_head(head)
                .map(|r| (r, rest))
                .ok_or(ReadError::Malformed);
        }
        if buf.len() > MAX_HEAD {
            return Err(ReadError::TooLarge);
        }
        let left = HEAD_DEADLINE.saturating_sub(start.elapsed());
        if left.is_zero() {
            return Err(ReadError::Timeout);
        }
        stream
            .set_read_timeout(Some(left.min(Duration::from_millis(500))))
            .map_err(ReadError::Io)?;
        match stream.read(&mut chunk) {
            Ok(0) => return Err(ReadError::Io(io::Error::from(io::ErrorKind::UnexpectedEof))),
            Ok(n) => buf.extend_from_slice(&chunk[..n]),
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(e) => return Err(ReadError::Io(e)),
        }
    }
}

fn find_head_end(buf: &[u8]) -> Option<usize> {
    buf.windows(4).position(|w| w == b"\r\n\r\n")
}

/// Parses a request head (request line + headers, without the blank line).
#[must_use]
pub fn parse_head(head: &str) -> Option<Request> {
    let mut lines = head.split("\r\n");
    let mut parts = lines.next()?.split(' ');
    let method = parts.next()?.to_string();
    let target = parts.next()?;
    let version = parts.next()?;
    if parts.next().is_some() || !version.starts_with("HTTP/1.") || method.is_empty() {
        return None;
    }
    if !method.bytes().all(|b| b.is_ascii_uppercase()) {
        return None;
    }
    let path = target.split(['?', '#']).next()?.to_string();
    let mut headers = Vec::new();
    for line in lines {
        let (name, value) = line.split_once(':')?;
        if name.is_empty() || name.contains(' ') {
            return None;
        }
        headers.push((name.to_ascii_lowercase(), value.trim().to_string()));
    }
    Some(Request {
        method,
        path,
        headers,
    })
}

/// Whether this is a valid WebSocket upgrade request; returns the accept
/// key, or the HTTP status to refuse with.
pub fn websocket_accept(req: &Request) -> Result<String, (u16, &'static str)> {
    if req.method != "GET" {
        return Err((405, "Method Not Allowed"));
    }
    if !req.header_has_token("upgrade", "websocket")
        || !req.header_has_token("connection", "upgrade")
    {
        return Err((400, "Bad Request"));
    }
    if req.header("sec-websocket-version") != Some("13") {
        return Err((426, "Upgrade Required"));
    }
    let key = req
        .header("sec-websocket-key")
        .ok_or((400, "Bad Request"))?;
    match crate::base64::decode(key.trim()) {
        Some(bytes) if bytes.len() == 16 => Ok(ws::accept_key(key)),
        _ => Err((400, "Bad Request")),
    }
}

/// Writes a complete response with a body.
pub fn respond(
    stream: &mut TcpStream,
    status: u16,
    reason: &str,
    headers: &[(&str, &str)],
    body: &[u8],
    head_only: bool,
) -> io::Result<()> {
    let mut out = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Length: {}\r\n",
        body.len()
    );
    for (n, v) in headers {
        out.push_str(n);
        out.push_str(": ");
        out.push_str(v);
        out.push_str("\r\n");
    }
    out.push_str("Connection: close\r\n\r\n");
    stream.write_all(out.as_bytes())?;
    if !head_only {
        stream.write_all(body)?;
    }
    stream.flush()
}

/// Writes the `101 Switching Protocols` answer.
pub fn respond_upgrade(stream: &mut TcpStream, accept: &str) -> io::Result<()> {
    let head = format!(
        "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
    );
    stream.write_all(head.as_bytes())?;
    stream.flush()
}

/// Decodes `%XX` escapes; `None` for malformed escapes, NUL bytes or
/// non-UTF-8 results.
#[must_use]
pub fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = bytes.get(i + 1..i + 3)?;
            let v = u8::from_str_radix(std::str::from_utf8(hex).ok()?, 16).ok()?;
            out.push(v);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    if out.contains(&0) {
        return None;
    }
    String::from_utf8(out).ok()
}

/// Maps a request path onto a file under `root`, refusing anything that
/// could escape it. Directories map to their `index.html`.
#[must_use]
pub fn resolve_static(root: &Path, url_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(url_path)?;
    if !decoded.starts_with('/') || decoded.contains('\\') {
        return None;
    }
    let mut rel = PathBuf::new();
    for comp in Path::new(&decoded[1..]).components() {
        match comp {
            Component::Normal(c) => rel.push(c),
            Component::CurDir => {}
            // `..`, a root or a prefix anywhere means "no".
            _ => return None,
        }
    }
    let root = root.canonicalize().ok()?;
    let mut path = root.join(rel);
    if path.is_dir() {
        path.push("index.html");
    }
    // Symlinks must not lead outside the root either.
    let real = path.canonicalize().ok()?;
    (real.starts_with(&root) && real.is_file()).then_some(real)
}

/// Content type for a file name.
#[must_use]
pub fn mime_type(path: &Path) -> &'static str {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("html" | "htm") => "text/html; charset=utf-8",
        Some("js" | "mjs") => "text/javascript; charset=utf-8",
        Some("css") => "text/css; charset=utf-8",
        Some("json" | "map") => "application/json",
        Some("webmanifest") => "application/manifest+json",
        Some("wasm") => "application/wasm",
        Some("svg") => "image/svg+xml",
        Some("png") => "image/png",
        Some("ico") => "image/x-icon",
        Some("txt") => "text/plain; charset=utf-8",
        _ => "application/octet-stream",
    }
}

/// Cache policy: entry points revalidate, everything else may be cached
/// briefly (the web build hashes its asset names).
#[must_use]
pub fn cache_control(path: &Path) -> &'static str {
    let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
    if name.ends_with(".html") || name == "sw.js" || name.ends_with(".webmanifest") {
        "no-cache"
    } else {
        "public, max-age=3600"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn req(head: &str) -> Request {
        parse_head(head).unwrap()
    }

    #[test]
    fn parses_heads() {
        let r = req("GET /ws?x=1 HTTP/1.1\r\nHost: a\r\nUpgrade: WebSocket\r\nConnection: keep-alive, Upgrade\r\nSec-WebSocket-Version: 13\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==");
        assert_eq!(r.method, "GET");
        assert_eq!(r.path, "/ws");
        assert_eq!(r.header("host"), Some("a"));
        assert!(r.header_has_token("connection", "upgrade"));
        assert_eq!(
            websocket_accept(&r).unwrap(),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn refuses_bad_upgrades() {
        let base = "GET /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n";
        let no_version = req(&format!(
            "{base}Sec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ=="
        ));
        assert_eq!(websocket_accept(&no_version).unwrap_err().0, 426);
        let bad_key = req(&format!(
            "{base}Sec-WebSocket-Version: 13\r\nSec-WebSocket-Key: abc"
        ));
        assert_eq!(websocket_accept(&bad_key).unwrap_err().0, 400);
        let post = req("POST /ws HTTP/1.1\r\nUpgrade: websocket\r\nConnection: Upgrade");
        assert_eq!(websocket_accept(&post).unwrap_err().0, 405);
        let plain = req("GET /ws HTTP/1.1\r\nConnection: keep-alive");
        assert_eq!(websocket_accept(&plain).unwrap_err().0, 400);
    }

    #[test]
    fn rejects_garbage_heads() {
        for bad in [
            "",
            "GET",
            "GET /",
            "GET / HTTP/1.1 extra",
            "get / HTTP/1.1",
            "GET / FTP/1.0",
            "GET / HTTP/1.1\r\nno-colon-here",
            "GET / HTTP/1.1\r\nbad name: v",
        ] {
            assert!(parse_head(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("/a%20b").unwrap(), "/a b");
        assert!(percent_decode("/a%2").is_none());
        assert!(percent_decode("/a%zz").is_none());
        assert!(percent_decode("/a%00b").is_none());
        assert!(percent_decode("/%ff").is_none());
    }

    #[test]
    fn static_paths_cannot_escape_the_root() {
        let dir = std::env::temp_dir().join(format!("p5-bridge-static-{}", std::process::id()));
        let root = dir.join("root");
        std::fs::create_dir_all(root.join("assets")).unwrap();
        std::fs::write(root.join("index.html"), "<p>hi</p>").unwrap();
        std::fs::write(root.join("assets/app.js"), "1").unwrap();
        std::fs::write(dir.join("secret.txt"), "no").unwrap();

        assert!(resolve_static(&root, "/").unwrap().ends_with("index.html"));
        assert!(resolve_static(&root, "/assets/app.js").is_some());
        assert!(resolve_static(&root, "/./assets/./app.js").is_some());
        for evil in [
            "/../secret.txt",
            "/assets/../../secret.txt",
            "/%2e%2e/secret.txt",
            "/%2E%2E%2Fsecret.txt",
            "/..%2fsecret.txt",
            "//etc/passwd",
            "/assets/..\\..\\secret.txt",
            "/assets%00/app.js",
            "relative",
            "/missing.js",
        ] {
            assert!(resolve_static(&root, evil).is_none(), "{evil}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.join("secret.txt"), root.join("link.txt")).unwrap();
            assert!(resolve_static(&root, "/link.txt").is_none());
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mime_types() {
        assert_eq!(mime_type(Path::new("a.wasm")), "application/wasm");
        assert_eq!(
            mime_type(Path::new("A.JS")),
            "text/javascript; charset=utf-8"
        );
        assert_eq!(cache_control(Path::new("index.html")), "no-cache");
        assert_eq!(cache_control(Path::new("sw.js")), "no-cache");
    }
}
