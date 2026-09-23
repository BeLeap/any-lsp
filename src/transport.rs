use crate::server::LspServer;
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

pub fn read_message<R: BufRead>(reader: &mut R) -> io::Result<Option<Value>> {
    let mut headers = HashMap::new();
    loop {
        let mut line = Vec::new();
        if reader.read_until(b'\n', &mut line)? == 0 {
            return Ok(None);
        }
        if line == b"\r\n" || line == b"\n" {
            break;
        }
        if let Some(separator) = line.iter().position(|byte| *byte == b':') {
            let key = &line[..separator];
            let value = &line[separator + 1..];
            headers.insert(
                String::from_utf8_lossy(key).to_ascii_lowercase(),
                String::from_utf8_lossy(value).trim().to_string(),
            );
        }
    }
    let Some(length) = headers
        .get("content-length")
        .and_then(|value| value.parse::<usize>().ok())
    else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "missing Content-Length",
        ));
    };
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    serde_json::from_slice(&body)
        .map(Some)
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))
}

pub fn write_message<W: Write>(writer: &mut W, message: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(message).map_err(io::Error::other)?;
    write!(writer, "Content-Length: {}\r\n\r\n", body.len())?;
    writer.write_all(&body)?;
    writer.flush()
}

pub fn serve<R: BufRead, W: Write>(
    server: &mut LspServer,
    reader: &mut R,
    writer: &mut W,
) -> io::Result<()> {
    loop {
        let Some(request) = read_message(reader)? else {
            return Ok(());
        };
        let method = request.get("method").and_then(Value::as_str).unwrap_or("");
        let result = server.handle(method, request.get("params").unwrap_or(&Value::Null));
        if let Some(id) = request.get("id") {
            write_message(
                writer,
                &json!({"jsonrpc": "2.0", "id": id, "result": result}),
            )?;
        }
        if method == "exit" {
            return Ok(());
        }
    }
}

pub fn run_stdio(server: &mut LspServer) -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut reader = io::BufReader::new(stdin.lock());
    let mut writer = io::BufWriter::new(stdout.lock());
    serve(server, &mut reader, &mut writer)
}
