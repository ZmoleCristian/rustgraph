use std::io::{BufRead, BufReader, Read, Write};
use std::process::{ChildStdin, ChildStdout};

type DynError = Box<dyn std::error::Error>;

/// Hard cap on a single LSP frame payload, in bytes. 64 MiB is well above any
/// legitimate rust-analyzer response and protects against accidental or
/// malicious oversize allocations.
pub(super) const MAX_LSP_PAYLOAD_BYTES: usize = 64 * 1024 * 1024;

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

/// Validate that an advertised LSP payload size is within the hard cap.
///
/// Returns an error tagged with both the offending size and the cap so the
/// failure is diagnosable from logs.
fn validate_payload_size(len: usize) -> Result<(), DynError> {
    if len > MAX_LSP_PAYLOAD_BYTES {
        return Err(format!(
            "rust-analyzer LSP frame too large: {len} bytes exceeds cap of {MAX_LSP_PAYLOAD_BYTES} bytes"
        )
        .into());
    }
    Ok(())
}

pub(super) fn read_message(
    stdout: &mut BufReader<ChildStdout>,
) -> Result<serde_json::Value, DynError> {
    let len = read_content_length(stdout)?;
    validate_payload_size(len)?;
    let mut payload = vec![0u8; len];
    stdout.read_exact(&mut payload)?;
    let value = serde_json::from_slice::<serde_json::Value>(&payload)?;
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::{MAX_LSP_PAYLOAD_BYTES, validate_payload_size};

    #[test]
    fn payload_cap_is_64_mib() {
        assert_eq!(MAX_LSP_PAYLOAD_BYTES, 64 * 1024 * 1024);
    }

    #[test]
    fn validate_payload_size_accepts_zero() {
        validate_payload_size(0).expect("zero is fine");
    }

    #[test]
    fn validate_payload_size_accepts_at_cap() {
        validate_payload_size(MAX_LSP_PAYLOAD_BYTES).expect("at cap is fine");
    }

    #[test]
    fn validate_payload_size_rejects_above_cap() {
        let err = validate_payload_size(MAX_LSP_PAYLOAD_BYTES + 1)
            .err()
            .expect("must reject");
        let msg = err.to_string();
        assert!(msg.contains("too large"), "msg: {msg}");
        assert!(msg.contains(&MAX_LSP_PAYLOAD_BYTES.to_string()), "msg: {msg}");
    }
}
