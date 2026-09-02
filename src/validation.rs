use crate::errors::AppError;

// validate that a string is not empty and within length limits
pub fn validate_string(
    value: &str,
    field_name: &str,
    min_len: usize,
    max_len: usize,
) -> Result<(), AppError> {
    // count characters, not bytes — a bengali name is ~3 bytes per character
    // and postgres VARCHAR(n) counts characters too
    let trimmed = value.trim();
    let length = trimmed.chars().count();
    if length < min_len {
        return Err(AppError::BadRequest(format!(
            "{} must be at least {} characters",
            field_name, min_len
        )));
    }
    if length > max_len {
        return Err(AppError::BadRequest(format!(
            "{} must be at most {} characters",
            field_name, max_len
        )));
    }
    Ok(())
}

// validate email has basic structure
pub fn validate_email(email: &str) -> Result<(), AppError> {
    if !email.contains('@') || !email.contains('.') {
        return Err(AppError::BadRequest("Invalid email format".to_string()));
    }
    Ok(())
}

// validate that a url starts with http/https
pub fn validate_url(url: &str, field_name: &str) -> Result<(), AppError> {
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(AppError::BadRequest(format!(
            "{} must be a valid URL starting with http:// or https://",
            field_name
        )));
    }
    Ok(())
}

// parses a datetime from a request body
// main format is YYYY-MM-DDTHH:MM:SS, but we accept a few common variants too
// an empty string gives back None so callers can clear the field
pub fn parse_datetime(
    value: &str,
    field_name: &str,
) -> Result<Option<chrono::NaiveDateTime>, AppError> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Ok(None);
    }

    const FORMATS: &[&str] = &[
        "%Y-%m-%dT%H:%M:%S",
        "%Y-%m-%dT%H:%M",
        "%Y-%m-%d %H:%M:%S",
        "%Y-%m-%d %H:%M",
    ];

    for fmt in FORMATS {
        if let Ok(dt) = chrono::NaiveDateTime::parse_from_str(trimmed, fmt) {
            return Ok(Some(dt));
        }
    }

    // rfc3339 with an offset like 2026-01-01T10:00:00Z — convert to naive utc
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(trimmed) {
        return Ok(Some(dt.naive_utc()));
    }

    Err(AppError::BadRequest(format!(
        "Invalid {} format (expected YYYY-MM-DDTHH:MM:SS)",
        field_name
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_bad_request(r: Result<(), AppError>) -> bool {
        matches!(r, Err(AppError::BadRequest(_)))
    }

    #[test]
    fn string_length_counts_characters_not_bytes() {
        // this was the bug: .len() is bytes, so a bengali name hit the limit at
        // about a third of its real length while postgres accepted it fine
        let bengali = "মোহাম্মদ নীলয় চন্দ্র দেব রায় চৌধুরী মজুমদার";
        assert!(
            bengali.len() > 100,
            "fixture must exceed 100 bytes to be meaningful"
        );
        assert!(bengali.chars().count() <= 100);
        assert!(validate_string(bengali, "Name", 2, 100).is_ok());
    }

    #[test]
    fn string_limits_are_enforced_at_the_edges() {
        assert!(is_bad_request(validate_string("", "Name", 2, 100)));
        assert!(is_bad_request(validate_string("a", "Name", 2, 100)));
        assert!(validate_string("ab", "Name", 2, 100).is_ok());
        assert!(validate_string(&"a".repeat(100), "Name", 2, 100).is_ok());
        assert!(is_bad_request(validate_string(
            &"a".repeat(101),
            "Name",
            2,
            100
        )));
        // whitespace is trimmed before measuring
        assert!(is_bad_request(validate_string("   ", "Name", 2, 100)));
        assert!(validate_string("  ab  ", "Name", 2, 100).is_ok());
    }

    #[test]
    fn urls_must_be_http_or_https() {
        assert!(validate_url("https://codeforces.com/contest/1920", "Link").is_ok());
        assert!(validate_url("http://example.com", "Link").is_ok());
        // these end up in an href, so they must never pass
        for bad in [
            "javascript:alert(document.cookie)",
            "data:text/html,<script>alert(1)</script>",
            "ftp://example.com/f",
            "//example.com",
            "example.com",
        ] {
            assert!(
                is_bad_request(validate_url(bad, "Link")),
                "should reject {bad}"
            );
        }
    }

    #[test]
    fn datetime_accepts_the_documented_formats() {
        let expected = "2026-09-01 18:00:00";
        for input in [
            "2026-09-01T18:00:00",
            "2026-09-01 18:00:00",
            "2026-09-01T18:00:00Z",
        ] {
            let got = parse_datetime(input, "d").unwrap().unwrap();
            assert_eq!(got.to_string(), expected, "for {input}");
        }
        // without seconds
        assert_eq!(
            parse_datetime("2026-09-01T18:00", "d")
                .unwrap()
                .unwrap()
                .to_string(),
            expected
        );
    }

    #[test]
    fn datetime_normalises_an_offset_to_utc() {
        // dhaka is +06:00
        assert_eq!(
            parse_datetime("2026-09-02T00:00:00+06:00", "d")
                .unwrap()
                .unwrap()
                .to_string(),
            "2026-09-01 18:00:00"
        );
    }

    #[test]
    fn empty_datetime_clears_the_field() {
        assert!(parse_datetime("", "d").unwrap().is_none());
        assert!(parse_datetime("   ", "d").unwrap().is_none());
    }

    #[test]
    fn malformed_datetime_is_rejected_not_silently_dropped() {
        // the original bug: these used to be stored as NULL with a 200
        for bad in [
            "not-a-date",
            "2026-13-45T00:00:00",
            "01/09/2026",
            "2026-09-01",
        ] {
            assert!(parse_datetime(bad, "d").is_err(), "should reject {bad}");
        }
    }

    #[test]
    fn datetime_accepts_javascript_iso_output() {
        // Date.prototype.toISOString() emits milliseconds; rfc3339 allows a
        // fractional part, so these parse rather than 400
        assert_eq!(
            parse_datetime("2026-09-01T18:00:00.000Z", "d")
                .unwrap()
                .unwrap()
                .to_string(),
            "2026-09-01 18:00:00"
        );
        assert!(parse_datetime("2026-09-01T18:00:00.123456Z", "d")
            .unwrap()
            .is_some());
    }

    #[test]
    fn email_needs_at_and_dot() {
        assert!(validate_email("2021331083@student.sust.edu").is_ok());
        assert!(is_bad_request(validate_email("no-at-sign.com")));
        assert!(is_bad_request(validate_email("no-dot@example")));
    }
}
