//! WebSocket (RFC 6455) server-side framing: the handshake accept value, a
//! frame parser over a growing byte buffer, a frame encoder, and message
//! reassembly with the protocol's rules enforced.
//!
//! Only what the bridge needs: text messages, ping/pong, close. Binary
//! messages are refused with close code 1003. No extensions.

use crate::{base64, sha1};

/// The GUID appended to the client key (RFC 6455 §1.3).
pub const ACCEPT_GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";

/// Largest message (after reassembly) the bridge accepts.
pub const MAX_MESSAGE: usize = 64 * 1024;

/// `Sec-WebSocket-Accept` for a client's `Sec-WebSocket-Key`.
#[must_use]
pub fn accept_key(client_key: &str) -> String {
    let mut s = client_key.trim().to_string();
    s.push_str(ACCEPT_GUID);
    base64::encode(&sha1::sha1(s.as_bytes()))
}

/// Frame opcodes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Opcode {
    /// Continuation of a fragmented message.
    Continuation,
    /// UTF-8 text.
    Text,
    /// Binary data.
    Binary,
    /// Connection close.
    Close,
    /// Ping.
    Ping,
    /// Pong.
    Pong,
}

impl Opcode {
    fn from_bits(bits: u8) -> Option<Self> {
        Some(match bits {
            0x0 => Self::Continuation,
            0x1 => Self::Text,
            0x2 => Self::Binary,
            0x8 => Self::Close,
            0x9 => Self::Ping,
            0xA => Self::Pong,
            _ => return None,
        })
    }

    fn bits(self) -> u8 {
        match self {
            Self::Continuation => 0x0,
            Self::Text => 0x1,
            Self::Binary => 0x2,
            Self::Close => 0x8,
            Self::Ping => 0x9,
            Self::Pong => 0xA,
        }
    }

    /// Control frames: close, ping, pong.
    #[must_use]
    pub fn is_control(self) -> bool {
        matches!(self, Self::Close | Self::Ping | Self::Pong)
    }
}

/// One decoded (unmasked) frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    /// Final fragment of a message.
    pub fin: bool,
    /// What it is.
    pub opcode: Opcode,
    /// Unmasked payload.
    pub payload: Vec<u8>,
}

/// A protocol violation, with the close code to send.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ProtocolError {
    /// RFC 6455 §7.4.1 status code.
    pub code: u16,
    /// Short reason for logs and the close frame.
    pub reason: &'static str,
}

const fn err(code: u16, reason: &'static str) -> ProtocolError {
    ProtocolError { code, reason }
}

/// Tries to parse one client frame from the front of `buf`. Returns
/// `Ok(None)` if more bytes are needed; consumed bytes are removed from
/// `buf` only when a whole frame is returned.
pub fn parse_client_frame(buf: &mut Vec<u8>) -> Result<Option<Frame>, ProtocolError> {
    if buf.len() < 2 {
        return Ok(None);
    }
    let b0 = buf[0];
    let b1 = buf[1];
    if b0 & 0x70 != 0 {
        return Err(err(1002, "reserved bits set"));
    }
    let fin = b0 & 0x80 != 0;
    let opcode = Opcode::from_bits(b0 & 0x0F).ok_or(err(1002, "unknown opcode"))?;
    if b1 & 0x80 == 0 {
        return Err(err(1002, "client frames must be masked"));
    }
    let mut pos = 2usize;
    let len7 = (b1 & 0x7F) as u64;
    let len = match len7 {
        126 => {
            if buf.len() < pos + 2 {
                return Ok(None);
            }
            let l = u64::from(u16::from_be_bytes([buf[2], buf[3]]));
            pos += 2;
            if l < 126 {
                return Err(err(1002, "non-minimal length"));
            }
            l
        }
        127 => {
            if buf.len() < pos + 8 {
                return Ok(None);
            }
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[2..10]);
            let l = u64::from_be_bytes(b);
            pos += 8;
            if l >> 63 != 0 {
                return Err(err(1002, "length has the high bit set"));
            }
            if l <= 0xFFFF {
                return Err(err(1002, "non-minimal length"));
            }
            l
        }
        l => l,
    };
    if opcode.is_control() {
        if !fin {
            return Err(err(1002, "fragmented control frame"));
        }
        if len > 125 {
            return Err(err(1002, "control frame too long"));
        }
    }
    if len > MAX_MESSAGE as u64 {
        return Err(err(1009, "message too big"));
    }
    let len = len as usize;
    if buf.len() < pos + 4 + len {
        return Ok(None);
    }
    let mask = [buf[pos], buf[pos + 1], buf[pos + 2], buf[pos + 3]];
    pos += 4;
    let payload: Vec<u8> = buf[pos..pos + len]
        .iter()
        .enumerate()
        .map(|(i, b)| b ^ mask[i % 4])
        .collect();
    buf.drain(..pos + len);
    Ok(Some(Frame {
        fin,
        opcode,
        payload,
    }))
}

/// Encodes an unmasked server frame.
#[must_use]
pub fn encode_frame(opcode: Opcode, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 10);
    out.push(0x80 | opcode.bits());
    let len = payload.len();
    if len < 126 {
        out.push(len as u8);
    } else if len <= 0xFFFF {
        out.push(126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    out.extend_from_slice(payload);
    out
}

/// A text frame.
#[must_use]
pub fn text_frame(text: &str) -> Vec<u8> {
    encode_frame(Opcode::Text, text.as_bytes())
}

/// A close frame with a status code and reason.
#[must_use]
pub fn close_frame(code: u16, reason: &str) -> Vec<u8> {
    let mut payload = code.to_be_bytes().to_vec();
    let reason = reason.as_bytes();
    payload.extend_from_slice(&reason[..reason.len().min(123)]);
    encode_frame(Opcode::Close, &payload)
}

/// What the connection should do after a frame.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    /// A complete text message.
    Text(String),
    /// Reply with a pong carrying this payload.
    Ping(Vec<u8>),
    /// The peer started (or answered) the close handshake; echo `code`
    /// and stop.
    Close(u16),
    /// Nothing to do (a pong, or a fragment of a longer message).
    None,
}

/// Reassembles fragmented messages and applies RFC 6455's message rules.
#[derive(Debug, Default)]
pub struct Assembler {
    partial: Option<Vec<u8>>,
}

impl Assembler {
    /// Feeds one frame.
    pub fn push(&mut self, frame: Frame) -> Result<Event, ProtocolError> {
        match frame.opcode {
            Opcode::Ping => Ok(Event::Ping(frame.payload)),
            Opcode::Pong => Ok(Event::None),
            Opcode::Close => {
                let code = match frame.payload.len() {
                    0 => 1000,
                    1 => return Err(err(1002, "bad close payload")),
                    _ => {
                        let c = u16::from_be_bytes([frame.payload[0], frame.payload[1]]);
                        if std::str::from_utf8(&frame.payload[2..]).is_err() {
                            return Err(err(1007, "close reason is not UTF-8"));
                        }
                        let valid = matches!(c, 1000..=1003 | 1007..=1011 | 3000..=4999);
                        if !valid {
                            return Err(err(1002, "invalid close code"));
                        }
                        c
                    }
                };
                Ok(Event::Close(code))
            }
            Opcode::Binary => Err(err(1003, "binary messages are not supported")),
            Opcode::Text => {
                if self.partial.is_some() {
                    return Err(err(1002, "new message before the previous ended"));
                }
                if frame.fin {
                    Self::finish(frame.payload)
                } else {
                    self.partial = Some(frame.payload);
                    Ok(Event::None)
                }
            }
            Opcode::Continuation => {
                let Some(mut buf) = self.partial.take() else {
                    return Err(err(1002, "continuation without a message"));
                };
                if buf.len() + frame.payload.len() > MAX_MESSAGE {
                    return Err(err(1009, "message too big"));
                }
                buf.extend_from_slice(&frame.payload);
                if frame.fin {
                    Self::finish(buf)
                } else {
                    self.partial = Some(buf);
                    Ok(Event::None)
                }
            }
        }
    }

    fn finish(bytes: Vec<u8>) -> Result<Event, ProtocolError> {
        String::from_utf8(bytes)
            .map(Event::Text)
            .map_err(|_| err(1007, "text is not UTF-8"))
    }
}

/// Builds a masked client frame (for tests and the bundled test client).
#[must_use]
pub fn client_frame(opcode: Opcode, fin: bool, payload: &[u8], mask: [u8; 4]) -> Vec<u8> {
    let mut out = Vec::with_capacity(payload.len() + 14);
    out.push(if fin { 0x80 } else { 0 } | opcode.bits());
    let len = payload.len();
    if len < 126 {
        out.push(0x80 | len as u8);
    } else if len <= 0xFFFF {
        out.push(0x80 | 126);
        out.extend_from_slice(&(len as u16).to_be_bytes());
    } else {
        out.push(0x80 | 127);
        out.extend_from_slice(&(len as u64).to_be_bytes());
    }
    out.extend_from_slice(&mask);
    out.extend(payload.iter().enumerate().map(|(i, b)| b ^ mask[i % 4]));
    out
}

/// Parses one unmasked server frame (for tests and the bundled client).
pub fn parse_server_frame(buf: &mut Vec<u8>) -> Option<Frame> {
    if buf.len() < 2 {
        return None;
    }
    let fin = buf[0] & 0x80 != 0;
    let opcode = Opcode::from_bits(buf[0] & 0x0F)?;
    let mut pos = 2;
    let len = match buf[1] & 0x7F {
        126 => {
            if buf.len() < 4 {
                return None;
            }
            pos = 4;
            usize::from(u16::from_be_bytes([buf[2], buf[3]]))
        }
        127 => {
            if buf.len() < 10 {
                return None;
            }
            pos = 10;
            let mut b = [0u8; 8];
            b.copy_from_slice(&buf[2..10]);
            u64::from_be_bytes(b) as usize
        }
        l => usize::from(l),
    };
    if buf.len() < pos + len {
        return None;
    }
    let payload = buf[pos..pos + len].to_vec();
    buf.drain(..pos + len);
    Some(Frame {
        fin,
        opcode,
        payload,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const MASK: [u8; 4] = [0x37, 0xfa, 0x21, 0x3d];

    #[test]
    fn rfc_6455_accept_example() {
        assert_eq!(
            accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn rfc_6455_masked_hello() {
        // §5.7: a single-frame masked text message containing "Hello".
        let mut buf = vec![
            0x81, 0x85, 0x37, 0xfa, 0x21, 0x3d, 0x7f, 0x9f, 0x4d, 0x51, 0x58,
        ];
        let f = parse_client_frame(&mut buf).unwrap().unwrap();
        assert!(f.fin);
        assert_eq!(f.opcode, Opcode::Text);
        assert_eq!(f.payload, b"Hello");
        assert!(buf.is_empty());
        // And the unmasked server form.
        assert_eq!(
            text_frame("Hello"),
            vec![0x81, 0x05, 0x48, 0x65, 0x6c, 0x6c, 0x6f]
        );
    }

    #[test]
    fn partial_input_waits() {
        let full = client_frame(Opcode::Text, true, &[b'a'; 300], MASK);
        for cut in 0..full.len() {
            let mut buf = full[..cut].to_vec();
            assert_eq!(parse_client_frame(&mut buf), Ok(None), "cut {cut}");
            assert_eq!(buf.len(), cut, "nothing consumed");
        }
        let mut buf = full.clone();
        buf.extend_from_slice(&client_frame(Opcode::Ping, true, b"x", MASK));
        let f = parse_client_frame(&mut buf).unwrap().unwrap();
        assert_eq!(f.payload.len(), 300);
        let p = parse_client_frame(&mut buf).unwrap().unwrap();
        assert_eq!(p.opcode, Opcode::Ping);
    }

    #[test]
    fn lengths_round_trip() {
        for n in [0usize, 125, 126, 65_535, 65_536] {
            let payload = vec![7u8; n];
            if n <= MAX_MESSAGE {
                let mut buf = client_frame(Opcode::Text, true, &payload, MASK);
                let f = parse_client_frame(&mut buf).unwrap().unwrap();
                assert_eq!(f.payload.len(), n);
            }
            let mut s = encode_frame(Opcode::Text, &payload);
            assert_eq!(parse_server_frame(&mut s).unwrap().payload.len(), n);
        }
    }

    #[test]
    fn protocol_violations() {
        let mut unmasked = vec![0x81, 0x01, b'a'];
        assert_eq!(parse_client_frame(&mut unmasked).unwrap_err().code, 1002);
        let mut rsv = client_frame(Opcode::Text, true, b"a", MASK);
        rsv[0] |= 0x40;
        assert_eq!(parse_client_frame(&mut rsv).unwrap_err().code, 1002);
        let mut big_ping = client_frame(Opcode::Ping, true, &[0; 126], MASK);
        assert_eq!(parse_client_frame(&mut big_ping).unwrap_err().code, 1002);
        let mut frag_ping = client_frame(Opcode::Ping, false, b"a", MASK);
        assert_eq!(parse_client_frame(&mut frag_ping).unwrap_err().code, 1002);
        let mut too_big = client_frame(Opcode::Text, true, &vec![0; MAX_MESSAGE + 1], MASK);
        assert_eq!(parse_client_frame(&mut too_big).unwrap_err().code, 1009);
        let mut bad_op = vec![0x83, 0x80, 0, 0, 0, 0];
        assert_eq!(parse_client_frame(&mut bad_op).unwrap_err().code, 1002);
        // 64-bit length with the high bit set.
        let mut hb = vec![0x81, 0xFF, 0x80, 0, 0, 0, 0, 0, 0, 0];
        assert_eq!(parse_client_frame(&mut hb).unwrap_err().code, 1002);
    }

    #[test]
    fn fragmented_text_reassembles_with_interleaved_ping() {
        let mut a = Assembler::default();
        let part = |op, fin, p: &[u8]| Frame {
            fin,
            opcode: op,
            payload: p.to_vec(),
        };
        assert_eq!(a.push(part(Opcode::Text, false, b"Hel")), Ok(Event::None));
        assert_eq!(
            a.push(part(Opcode::Ping, true, b"p")),
            Ok(Event::Ping(b"p".to_vec()))
        );
        assert_eq!(
            a.push(part(Opcode::Continuation, true, b"lo")),
            Ok(Event::Text("Hello".into()))
        );
        assert!(a.push(part(Opcode::Continuation, true, b"x")).is_err());
        assert_eq!(
            a.push(part(Opcode::Binary, true, b"x")).unwrap_err().code,
            1003
        );
        assert_eq!(
            a.push(part(Opcode::Text, true, &[0xff, 0xfe]))
                .unwrap_err()
                .code,
            1007
        );
    }

    #[test]
    fn close_codes() {
        let mut a = Assembler::default();
        let close = |p: &[u8]| Frame {
            fin: true,
            opcode: Opcode::Close,
            payload: p.to_vec(),
        };
        assert_eq!(a.push(close(&[])), Ok(Event::Close(1000)));
        assert_eq!(a.push(close(&[0x03, 0xE9])), Ok(Event::Close(1001)));
        assert!(a.push(close(&[0x03])).is_err());
        assert!(a.push(close(&[0x03, 0xEC])).is_err()); // 1004 reserved
        assert!(a.push(close(&[0x03, 0xE8, 0xff])).is_err());
        let cf = close_frame(1000, "bye");
        assert_eq!(&cf[..4], &[0x88, 5, 0x03, 0xE8]);
    }
}
