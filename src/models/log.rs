use serde::{Deserialize, Serialize};
use sqlx::FromRow;
use chrono::{DateTime, Utc};

#[derive(Debug, Serialize, Deserialize, FromRow)]
pub struct AuditLog {
    pub id: i64,
    pub admin_username: String,
    pub action: String,
    pub target: Option<String>,
    pub details: Option<String>,
    pub created_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CreateLogRequest {
    pub admin_username: String,
    pub action: String,
    pub target: Option<String>,
    pub details: Option<String>,
}
