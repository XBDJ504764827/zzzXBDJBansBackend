use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use crate::AppState;
use crate::models::ban::{Ban, CreateBanRequest, UpdateBanRequest};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct BanFilter {
    steam_id: Option<String>,
    ip: Option<String>,
}

pub async fn list_bans(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let bans = sqlx::query_as::<_, Ban>("SELECT * FROM bans ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await;

    match bans {
        Ok(data) => (StatusCode::OK, Json(data)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn check_ban(
    State(state): State<Arc<AppState>>,
    Query(params): Query<BanFilter>,
) -> impl IntoResponse {
    if params.steam_id.is_none() && params.ip.is_none() {
        return (StatusCode::BAD_REQUEST, "Missing steam_id or ip").into_response();
    }
    
    // Check active bans
    let steam_id = params.steam_id.unwrap_or_default();
    let ip = params.ip.unwrap_or_default();
    
    // Logic: Find any active ban matching SteamID or IP
    let ban = sqlx::query_as::<_, Ban>(
        "SELECT * FROM bans WHERE status = 'active' AND (steam_id = ? OR ip = ?) LIMIT 1"
    )
    .bind(&steam_id)
    .bind(&ip)
    .fetch_optional(&state.db)
    .await;

    match ban {
        Ok(Some(b)) => (StatusCode::OK, Json(b)).into_response(), // Banned
        Ok(None) => (StatusCode::NOT_FOUND, Json("Not banned")).into_response(), // Not banned
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn create_ban(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateBanRequest>,
) -> impl IntoResponse {
    let result = sqlx::query(
        "INSERT INTO bans (name, steam_id, ip, ban_type, reason, duration, admin_name) VALUES (?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(payload.name)
    .bind(payload.steam_id)
    .bind(payload.ip)
    .bind(payload.ban_type)
    .bind(payload.reason)
    .bind(payload.duration)
    .bind(payload.admin_name)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => (StatusCode::CREATED, Json("Ban created")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn update_ban(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateBanRequest>,
) -> impl IntoResponse {
    if let Some(status) = payload.status {
        let _ = sqlx::query("UPDATE bans SET status = ? WHERE id = ?")
            .bind(status).bind(id)
            .execute(&state.db).await;
    }
    if let Some(reason) = payload.reason {
         let _ = sqlx::query("UPDATE bans SET reason = ? WHERE id = ?")
            .bind(reason).bind(id)
            .execute(&state.db).await;
    }
    // duration update if needed...

    (StatusCode::OK, Json("Ban updated")).into_response()
}

pub async fn delete_ban(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    // Soft delete usually via status update, but API requested delete.
    let result = sqlx::query("DELETE FROM bans WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => (StatusCode::OK, Json("Ban deleted")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
