use sqlx::mysql::{MySqlPool, MySqlPoolOptions};
use std::env;

pub async fn establish_connection() -> MySqlPool {
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set");
    
    MySqlPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .expect("Failed to create pool")
}
