use serde::{Deserialize, Serialize};
use sqlx::FromRow;

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct Whitelist {
    pub id: i64,
    pub steam_id: String,
    pub steam_id_3: Option<String>,
    pub steam_id_64: Option<String>,
    pub name: String,
    pub status: String, // 'approved', 'pending', 'rejected'
    pub created_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct CreateWhitelistRequest {
    pub steam_id: String,
    pub name: String,
}
