use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout};

type DynError = Box<dyn std::error::Error>;

pub(super) fn write_message(
    stdin: &mut ChildStdin,
    value: &serde_json::Value,
) -> Result<(), DynError> {
    let payload = serde_json::to_vec(value)?;
    write!(stdin, "Content-Length: {}\r\n\r\n", payload.len())?;
    stdin.write_all(&payload)?;
    stdin.flush()?;
    Ok(())
}

fn read_content_length(stdout: &mut BufReader<ChildStdout>) -> Result<usize, DynError> {
    let mut content_length: Option<usize> = None;
    loop {
        let mut line = String::new();
        let read = stdout.read_line(&mut line)?;
        if read == 0 {
            return Err("rust-analyzer closed the LSP stream unexpectedly".into());
        }
        let trimmed = line.trim_end();
        if trimmed.is_empty() {
            break;
        }
        if trimmed.to_ascii_lowercase().starts_with("content-length:") {
            let raw_len = trimmed
                .split_once(':')
                .map(|(_, value)| value)
                .unwrap_or("");
            content_length = raw_len.trim().parse::<usize>().ok();
        }
    }

    content_length.ok_or_else(|| "missing Content-Length header from rust-analyzer".into())
}

pub(super) fn read_message(
    stdout: &mut BufReader<ChildStdout>,
) -> Result<serde_json::Value, DynError> {
    let len = read_content_length(stdout)?;
    let mut payload = vec![0u8; len];
    stdout.read_exact(&mut payload)?;
    let value = serde_json::from_slice::<serde_json::Value>(&payload)?;
    Ok(value)
}
