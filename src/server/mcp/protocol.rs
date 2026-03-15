use anyhow::{Context, Result};
use serde_json::{json, Value};
use std::io::{BufRead, Write};

pub(super) const JSONRPC_VERSION: &str = "2.0";
pub(super) const INVALID_REQUEST: i64 = -32_600;
pub(super) const METHOD_NOT_FOUND: i64 = -32_601;
pub(super) const INVALID_PARAMS: i64 = -32_602;
pub(super) const SERVER_NOT_INITIALIZED: i64 = -32_002;

pub(super) fn read_message(reader: &mut impl BufRead) -> Result<Option<Value>> {
    let mut content_length = None;
    let mut saw_header_bytes = false;

    loop {
        let mut header = String::new();
        let bytes = reader.read_line(&mut header)?;
        if bytes == 0 {
            if saw_header_bytes {
                anyhow::bail!("MCP transport closed while reading headers")
            }
            return Ok(None);
        }

        saw_header_bytes = true;
        let header = header.trim();
        if header.is_empty() {
            break;
        }

        if let Some((name, value)) = header.split_once(':') {
            if name.eq_ignore_ascii_case("content-length") {
                content_length = Some(
                    value
                        .trim()
                        .parse::<usize>()
                        .context("invalid Content-Length header")?,
                );
            }
        }
    }

    let length = content_length.ok_or_else(|| anyhow::anyhow!("Missing Content-Length header"))?;
    let mut buffer = vec![0u8; length];
    reader.read_exact(&mut buffer)?;
    Ok(Some(
        serde_json::from_slice(&buffer).context("failed to parse MCP message body")?,
    ))
}

pub(super) fn write_message(writer: &mut impl Write, message: &Value) -> Result<()> {
    let body = serde_json::to_vec(message)?;
    let header = format!("Content-Length: {}\r\n\r\n", body.len());
    writer.write_all(header.as_bytes())?;
    writer.write_all(&body)?;
    writer.flush()?;
    Ok(())
}

pub(super) fn success_response(id: Value, result: Value) -> Value {
    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id,
        "result": result,
    })
}

pub(super) fn error_response(
    id: Option<Value>,
    code: i64,
    message: impl Into<String>,
    data: Option<Value>,
) -> Value {
    let mut error = json!({
        "code": code,
        "message": message.into(),
    });

    if let Some(data) = data {
        error["data"] = data;
    }

    json!({
        "jsonrpc": JSONRPC_VERSION,
        "id": id.unwrap_or(Value::Null),
        "error": error,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn framed_messages_round_trip() {
        let message = json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "ping",
        });

        let mut bytes = Vec::new();
        write_message(&mut bytes, &message).unwrap();

        let mut cursor = Cursor::new(bytes);
        let decoded = read_message(&mut cursor).unwrap().unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn clean_eof_returns_none() {
        let mut cursor = Cursor::new(Vec::<u8>::new());
        assert!(read_message(&mut cursor).unwrap().is_none());
    }
}
