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
