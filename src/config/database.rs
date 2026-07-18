use sqlx::{postgres::PgPoolOptions, PgPool};

pub async fn connect() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env file");

    // explicit pool size — neon free tier works best with 5
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to connect to database");

    tracing::info!("connected to database");
    pool
}
