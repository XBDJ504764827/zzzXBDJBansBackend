use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct PlayerRecord {
    pub id: i64,
    pub player_name: String,
    pub steam_id: String,
    pub player_ip: String,
    pub server_name: Option<String>,
    pub server_address: Option<String>,
    pub connect_time: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateRecordRequest {
    pub player_name: String,
    pub steam_id: String,
    pub player_ip: String,
    pub server_name: Option<String>,
    pub server_address: Option<String>,
}
