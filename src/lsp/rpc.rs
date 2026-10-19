//! The wire format of the Language Server Protocol: JSON-RPC messages, each preceded by a
//! `Content-Length: N` header and an empty line (`\r\n\r\n`), on standard input and output.

use std::io::{self, BufRead, Write};

use crate::json::{obj, Json};

/// The largest message accepted (a document of a few megabytes is far below it).
const MAX_MESSAGE: usize = 64 << 20;

/// Reads one message. `Ok(None)` at the end of the input.
pub fn read(input: &mut impl BufRead) -> io::Result<Option<String>> {
    let mut length: Option<usize> = None;
    let mut line = String::new();
    loop {
        line.clear();
        if input.read_line(&mut line)? == 0 {
            return Ok(None);
        }
        let header = line.trim_end_matches(['\r', '\n']);
        if header.is_empty() {
            // a stray empty line before the headers is skipped
            if length.is_some() {
                break;
            }
            continue;
        }
        if let Some((name, value)) = header.split_once(':') {
            if name.trim().eq_ignore_ascii_case("content-length") {
                let n = value.trim().parse::<usize>().map_err(|_| bad(format!("bad Content-Length `{}`", value.trim())))?;
                if n > MAX_MESSAGE {
                    return Err(bad(format!("a message of {n} bytes is too large")));
                }
                length = Some(n);
            }
        }
    }
    let n = length.unwrap_or(0);
    let mut body = vec![0u8; n];
    input.read_exact(&mut body)?;
    String::from_utf8(body).map(Some).map_err(|_| bad("a message is not valid UTF-8".to_string()))
}

fn bad(msg: String) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, msg)
}

/// Writes one message.
pub fn write(out: &mut impl Write, msg: &Json) -> io::Result<()> {
    let body = msg.to_string();
    write!(out, "Content-Length: {}\r\n\r\n{body}", body.len())?;
    out.flush()
}

/// The reply to the request `id`.
pub fn reply(id: Json, result: Json) -> Json {
    obj([("jsonrpc", "2.0".into()), ("id", id), ("result", result)])
}

/// The error reply to the request `id`.
pub fn error(id: Json, code: i64, message: impl Into<String>) -> Json {
    obj([("jsonrpc", "2.0".into()), ("id", id), ("error", obj([("code", code.into()), ("message", message.into().into())]))])
}

/// A notification from the server.
pub fn notification(method: &str, params: Json) -> Json {
    obj([("jsonrpc", "2.0".into()), ("method", method.into()), ("params", params)])
}

pub const PARSE_ERROR: i64 = -32700;
pub const INVALID_REQUEST: i64 = -32600;
pub const METHOD_NOT_FOUND: i64 = -32601;
pub const INVALID_PARAMS: i64 = -32602;
pub const INTERNAL_ERROR: i64 = -32603;
/// A request before `initialize` (LSP).
pub const SERVER_NOT_INITIALIZED: i64 = -32002;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_framed_messages() {
        let text = "Content-Length: 2\r\n\r\n{}Content-Type: x\r\ncontent-length: 4\r\n\r\nnull";
        let mut input = io::BufReader::new(text.as_bytes());
        assert_eq!(read(&mut input).unwrap().as_deref(), Some("{}"));
        assert_eq!(read(&mut input).unwrap().as_deref(), Some("null"));
        assert_eq!(read(&mut input).unwrap(), None);
    }

    #[test]
    fn writes_the_length_in_bytes() {
        let mut out = Vec::new();
        write(&mut out, &Json::Str("é".into())).unwrap();
        assert_eq!(String::from_utf8(out).unwrap(), "Content-Length: 4\r\n\r\n\"é\"");
    }
}
