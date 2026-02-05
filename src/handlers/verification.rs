use axum::{
    extract::{State, Path},
    http::StatusCode,
    Json,
};
use serde::{Deserialize, Serialize};
use crate::AppState;
use std::sync::Arc;
use sqlx::Row;
use chrono::{DateTime, Utc};

#[derive(Serialize)]
pub struct VerificationRecord {
    pub steam_id: String,
    pub status: String,
    pub reason: Option<String>,
    pub steam_level: Option<i32>,
    pub playtime_minutes: Option<i32>,
    pub created_at: Option<DateTime<Utc>>,
    pub updated_at: Option<DateTime<Utc>>,
}

#[derive(Deserialize)]
pub struct CreateVerificationRequest {
    pub steam_id: String,
    pub status: Option<String>, // 'pending', 'allowed', 'denied'
    pub reason: Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateVerificationRequest {
    pub status: Option<String>,
    pub reason: Option<String>,
}

pub async fn list_verifications(
    State(state): State<Arc<AppState>>,
) -> Result<Json<Vec<VerificationRecord>>, String> {
    let rows = sqlx::query("SELECT steam_id, status, reason, steam_level, playtime_minutes, created_at, updated_at FROM player_verifications ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    let records = rows.into_iter().map(|row| VerificationRecord {
        steam_id: row.get("steam_id"),
        status: row.get("status"),
        reason: row.get("reason"),
        steam_level: row.get("steam_level"),
        playtime_minutes: row.get("playtime_minutes"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }).collect();

    Ok(Json(records))
}

pub async fn create_verification(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateVerificationRequest>,
) -> Result<Json<VerificationRecord>, String> {
    let status = payload.status.unwrap_or_else(|| "pending".to_string());
    
    // Check if exists
    let exists: bool = sqlx::query_scalar("SELECT COUNT(*) FROM player_verifications WHERE steam_id = ?")
        .bind(&payload.steam_id)
        .fetch_one(&state.db)
        .await
        .unwrap_or(0) > 0;

    if exists {
        return Err("Verification record already exists for this SteamID".to_string());
    }

    let _ = sqlx::query("INSERT INTO player_verifications (steam_id, status, reason) VALUES (?, ?, ?)")
        .bind(&payload.steam_id)
        .bind(&status)
        .bind(&payload.reason)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    // Return the created record (fetch it back or construct it)
    let row = sqlx::query("SELECT steam_id, status, reason, steam_level, playtime_minutes, created_at, updated_at FROM player_verifications WHERE steam_id = ?")
        .bind(&payload.steam_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(Json(VerificationRecord {
        steam_id: row.get("steam_id"),
        status: row.get("status"),
        reason: row.get("reason"),
        steam_level: row.get("steam_level"),
        playtime_minutes: row.get("playtime_minutes"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }))
}

pub async fn update_verification(
    State(state): State<Arc<AppState>>,
    Path(steam_id): Path<String>,
    Json(payload): Json<UpdateVerificationRequest>,
) -> Result<Json<VerificationRecord>, String> {
    if let Some(s) = &payload.status {
        let _ = sqlx::query("UPDATE player_verifications SET status = ? WHERE steam_id = ?")
            .bind(s)
            .bind(&steam_id)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
    }
    
    if let Some(r) = &payload.reason {
         let _ = sqlx::query("UPDATE player_verifications SET reason = ? WHERE steam_id = ?")
            .bind(r)
            .bind(&steam_id)
            .execute(&state.db)
            .await
            .map_err(|e| e.to_string())?;
    }

    // Return updated
    let row = sqlx::query("SELECT steam_id, status, reason, steam_level, playtime_minutes, created_at, updated_at FROM player_verifications WHERE steam_id = ?")
        .bind(&steam_id)
        .fetch_one(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(Json(VerificationRecord {
        steam_id: row.get("steam_id"),
        status: row.get("status"),
        reason: row.get("reason"),
        steam_level: row.get("steam_level"),
        playtime_minutes: row.get("playtime_minutes"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }))
}

pub async fn delete_verification(
    State(state): State<Arc<AppState>>,
    Path(steam_id): Path<String>,
) -> Result<StatusCode, String> {
    sqlx::query("DELETE FROM player_verifications WHERE steam_id = ?")
        .bind(steam_id)
        .execute(&state.db)
        .await
        .map_err(|e| e.to_string())?;

    Ok(StatusCode::NO_CONTENT)
}
