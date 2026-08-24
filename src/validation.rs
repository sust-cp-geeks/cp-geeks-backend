use crate::errors::AppError;

// validate that a string is not empty and within length limits
pub fn validate_string(
    value: &str,
    field_name: &str,
    min_len: usize,
    max_len: usize,
) -> Result<(), AppError> {
    let trimmed = value.trim();
    if trimmed.len() < min_len {
        return Err(AppError::BadRequest(format!(
            "{} must be at least {} characters",
            field_name, min_len
        )));
    }
    if trimmed.len() > max_len {
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
