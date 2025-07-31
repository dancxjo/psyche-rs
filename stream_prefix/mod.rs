use chrono::{DateTime, Local};
use std::str;

/// Parses a timestamp prefix in the form `@{<RFC3339 datetime>}` at the start of a buffer.
/// Returns the parsed `DateTime<Local>` and the number of bytes to skip if successful.
pub fn parse_timestamp_prefix(buf: &[u8]) -> Option<(DateTime<Local>, usize)> {
    if buf.starts_with(b"@{") {
        if let Some(end_brace) = buf.iter().position(|&b| b == b'}') {
            let ts_str = str::from_utf8(&buf[2..end_brace]).ok()?;
            let dt = DateTime::parse_from_rfc3339(ts_str).ok()?;
            let local_dt = dt.with_timezone(&Local);
            Some((local_dt, end_brace + 1))
        } else {
            None
        }
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_valid_prefix() {
        let ts = "2025-07-31T14:00:00-07:00";
        let data = format!("@{{{}}}", ts);
        let (parsed, used) = parse_timestamp_prefix(data.as_bytes()).unwrap();
        assert_eq!(used, data.len());
        assert_eq!(
            parsed,
            DateTime::parse_from_rfc3339(ts)
                .unwrap()
                .with_timezone(&Local)
        );
    }

    #[test]
    fn returns_none_for_malformed() {
        assert!(parse_timestamp_prefix(b"@{not a date}").is_none());
    }

    #[test]
    fn returns_none_when_missing_brace() {
        assert!(parse_timestamp_prefix(b"@{2025-01-01T00:00:00Z").is_none());
    }
}
