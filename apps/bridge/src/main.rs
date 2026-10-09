//! Command-line entry point. See `--help`.

use std::net::IpAddr;
use std::path::PathBuf;
use std::process::ExitCode;

use player5_bridge::{start, Config, SourceKind, DEFAULT_PORT};

const USAGE: &str = "\
player5-bridge: serve the booth's network clock to browsers over WebSocket

USAGE:
    player5-bridge [OPTIONS]

OPTIONS:
    --source <sim|prolink|opus|link>  Clock to follow (default: prolink)
    --sim-bpm <bpm>                    Tempo of the simulated clock (default: 120)
    --port <port>                      TCP port (default: 17505; 0 = any free port)
    --bind <address>                   Address to listen on (default: 0.0.0.0)
    --web <dir>                        Also serve the built web app from <dir>
    --allow-origin <origin>            Also let pages from <origin> use the WebSocket,
                                       e.g. https://example.org (repeatable; * = any)
    --device-number <n>                Pro DJ Link device number to claim (default: 5)
    --interface <ipv4>                 Booth-network interface address (default: discover)
    --passive                          Listen only; do not join the network as a device
    --prolink-port-base <port>         Testing: Pro DJ Link on 127.0.0.1 ports base..base+2
    --verbose                          Log every connection and source message
    -h, --help                         Print this help

Stop with Ctrl-C.";

fn parse(args: &[String]) -> Result<Config, String> {
    let mut config = Config::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match arg.as_str() {
            "-h" | "--help" => return Err(String::new()),
            "--source" => {
                let v = value("--source")?;
                config.source =
                    SourceKind::from_name(&v).ok_or_else(|| format!("unknown source {v:?}"))?;
            }
            "--sim-bpm" => {
                let v = value("--sim-bpm")?;
                let bpm: f64 = v.parse().map_err(|_| format!("bad --sim-bpm {v:?}"))?;
                if !(20.0..=400.0).contains(&bpm) {
                    return Err("--sim-bpm must be between 20 and 400".into());
                }
                config.options.sim_bpm = bpm;
            }
            "--port" => {
                let v = value("--port")?;
                config.port = v.parse().map_err(|_| format!("bad --port {v:?}"))?;
            }
            "--bind" => {
                let v = value("--bind")?;
                config.bind = v
                    .parse::<IpAddr>()
                    .map_err(|_| format!("bad --bind {v:?}"))?;
            }
            "--web" => {
                let dir = PathBuf::from(value("--web")?);
                if !dir.is_dir() {
                    return Err(format!("--web {} is not a directory", dir.display()));
                }
                config.web = Some(dir);
            }
            "--device-number" => {
                let v = value("--device-number")?;
                config.options.device_number = v
                    .parse::<u8>()
                    .ok()
                    .filter(|n| (1..=127).contains(n))
                    .ok_or_else(|| format!("bad --device-number {v:?}"))?;
            }
            "--interface" => {
                let v = value("--interface")?;
                config.options.interface =
                    Some(v.parse().map_err(|_| format!("bad --interface {v:?}"))?);
            }
            "--allow-origin" => {
                let v = value("--allow-origin")?;
                let v = v.trim().trim_end_matches('/').to_string();
                if v != "*" && !(v.starts_with("http://") || v.starts_with("https://")) {
                    return Err(format!(
                        "--allow-origin {v:?}: expected an origin like https://example.org, or *"
                    ));
                }
                config.allowed_origins.push(v);
            }
            "--passive" => config.options.passive = true,
            "--prolink-port-base" => {
                let v = value("--prolink-port-base")?;
                config.options.prolink_port_base = Some(
                    v.parse::<u16>()
                        .ok()
                        .filter(|p| *p > 0 && *p < 65_534)
                        .ok_or_else(|| format!("bad --prolink-port-base {v:?}"))?,
                );
            }
            "--verbose" => config.verbose = true,
            other => return Err(format!("unknown option {other:?}")),
        }
    }
    Ok(config)
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let config = match parse(&args) {
        Ok(c) => c,
        Err(e) if e.is_empty() => {
            println!("{USAGE}");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            eprintln!("player5-bridge: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    let web = config.web.clone();
    let server = match start(config) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("player5-bridge: cannot listen: {e}");
            return ExitCode::from(1);
        }
    };
    let addr = server.local_addr();
    eprintln!("player5-bridge: listening on {addr} (WebSocket at /ws)");
    if web.is_some() {
        eprintln!("player5-bridge: serving the web app at http://{addr}/");
    }
    if addr.port() != DEFAULT_PORT {
        eprintln!("player5-bridge: note: browsers default to port {DEFAULT_PORT}");
    }
    server.wait();
    ExitCode::SUCCESS
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(s: &str) -> Vec<String> {
        s.split_whitespace().map(String::from).collect()
    }

    #[test]
    fn parses_options() {
        let c = parse(&args(
            "--source sim --sim-bpm 128 --port 0 --bind 127.0.0.1 --passive",
        ))
        .unwrap();
        assert_eq!(c.source, SourceKind::Sim);
        assert_eq!(c.options.sim_bpm, 128.0);
        assert_eq!(c.port, 0);
        assert!(c.options.passive);
        assert!(parse(&args("--source nope")).is_err());
        let c = parse(&args("--allow-origin https://a.example/ --allow-origin *")).unwrap();
        assert_eq!(c.allowed_origins, ["https://a.example", "*"]);
        assert!(parse(&args("--allow-origin a.example")).is_err());
        assert!(parse(&args("--sim-bpm 1000")).is_err());
        assert!(parse(&args("--device-number 0")).is_err());
        assert!(parse(&args("--port")).is_err());
        assert_eq!(parse(&args("--help")).unwrap_err(), "");
    }
}
