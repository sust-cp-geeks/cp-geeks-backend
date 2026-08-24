use rand::Rng;
use sqlx::PgPool;

use crate::errors::AppError;

// generates a random 6-digit otp code
pub fn generate_otp() -> String {
    let mut rng = rand::thread_rng();
    let code: u32 = rng.gen_range(100_000..1_000_000);
    code.to_string()
}

// stores an otp in the database, invalidating any previous codes for the same email
pub async fn store_otp(pool: &PgPool, email: &str, code: &str) -> Result<(), AppError> {
    // mark all existing otps for this email as used (invalidate them)
    sqlx::query("UPDATE otp_codes SET used = true WHERE email = $1 AND used = false")
        .bind(email)
        .execute(pool)
        .await?;

    // insert the new otp — expires in 10 minutes
    sqlx::query(
        r#"INSERT INTO otp_codes (email, code, expires_at)
           VALUES ($1, $2, NOW() + INTERVAL '10 minutes')"#,
    )
    .bind(email)
    .bind(code)
    .execute(pool)
    .await?;

    Ok(())
}

// burns every outstanding code for an email — used after too many wrong guesses
pub async fn invalidate_otps(pool: &PgPool, email: &str) -> Result<(), AppError> {
    sqlx::query("UPDATE otp_codes SET used = true WHERE email = $1 AND used = false")
        .bind(email)
        .execute(pool)
        .await?;
    Ok(())
}

// verifies an otp — checks it exists, is not expired, and has not been used
pub async fn verify_otp(pool: &PgPool, email: &str, code: &str) -> Result<bool, AppError> {
    let result = sqlx::query_scalar::<_, i32>(
        r#"UPDATE otp_codes
           SET used = true
           WHERE email = $1
             AND code = $2
             AND used = false
             AND expires_at > NOW()
           RETURNING id"#,
    )
    .bind(email)
    .bind(code)
    .fetch_optional(pool)
    .await?;

    // if we got an id back, the otp was valid and is now marked as used
    Ok(result.is_some())
}
