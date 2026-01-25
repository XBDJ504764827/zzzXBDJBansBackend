use axum::{
    extract::{Extension, Path, Query, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use crate::AppState;
use crate::models::ban::{Ban, CreateBanRequest, UpdateBanRequest};
use crate::handlers::auth::Claims;
use crate::utils::log_admin_action;
use serde::Deserialize;

#[derive(Deserialize)]
pub struct BanFilter {
    steam_id: Option<String>,
    ip: Option<String>,
}

pub async fn list_bans(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Lazy expire check: Update all active bans that have expired
    let _ = sqlx::query("UPDATE bans SET status = 'expired' WHERE status = 'active' AND expires_at < NOW()")
        .execute(&state.db)
        .await;

    let bans = sqlx::query_as::<_, Ban>("SELECT * FROM bans ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await;

    match bans {
        Ok(data) => (StatusCode::OK, Json(data)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

use crate::utils::{calculate_expires_at, parse_duration};
use chrono::Utc;

// ... (imports)

// ... check_ban implementation
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
    
    let ban = sqlx::query_as::<_, Ban>(
        "SELECT * FROM bans WHERE status = 'active' AND (steam_id = ? OR ip = ?) LIMIT 1"
    )
    .bind(&steam_id)
    .bind(&ip)
    .fetch_optional(&state.db)
    .await;

    match ban {
        Ok(Some(mut b)) => {
            // Check expiration
            if let Some(expires_at) = b.expires_at {
                if Utc::now() > expires_at {
                    // Expired! Update DB
                    let _ = sqlx::query("UPDATE bans SET status = 'expired' WHERE id = ?")
                        .bind(b.id)
                        .execute(&state.db)
                        .await;
                    
                    return (StatusCode::NOT_FOUND, Json("Not banned (Expired)")).into_response();
                }
            }
            (StatusCode::OK, Json(b)).into_response()
        }, 
        Ok(None) => (StatusCode::NOT_FOUND, Json("Not banned")).into_response(), 
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn create_ban(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Json(payload): Json<CreateBanRequest>,
) -> impl IntoResponse {
    let expires_at = calculate_expires_at(&payload.duration);

    let result = sqlx::query(
        "INSERT INTO bans (name, steam_id, ip, ban_type, reason, duration, admin_name, expires_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?)"
    )
    .bind(&payload.name)
    .bind(&payload.steam_id)
    .bind(&payload.ip)
    .bind(&payload.ban_type)
    .bind(&payload.reason)
    .bind(&payload.duration)
    .bind(&payload.admin_name)
    .bind(expires_at)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {
            let _ = log_admin_action(
                &state.db, 
                &user.sub, 
                "create_ban", 
                &format!("User: {}, SteamID: {}", payload.name, payload.steam_id), 
                &format!("Reason: {}, Duration: {}", payload.reason.clone().unwrap_or_default(), payload.duration)
            ).await;
            (StatusCode::CREATED, Json("Ban created")).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn update_ban(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateBanRequest>,
) -> impl IntoResponse {
    if let Some(status) = payload.status {
        let _ = sqlx::query("UPDATE bans SET status = ? WHERE id = ?")
            .bind(status).bind(id)
            .execute(&state.db).await;
    }
    // ... (other fields name, steam_id etc same as before)
    if let Some(name) = payload.name {
         let _ = sqlx::query("UPDATE bans SET name = ? WHERE id = ?")
            .bind(name).bind(id)
            .execute(&state.db).await;
    }
    if let Some(steam_id) = payload.steam_id {
         let _ = sqlx::query("UPDATE bans SET steam_id = ? WHERE id = ?")
            .bind(steam_id).bind(id)
            .execute(&state.db).await;
    }
    if let Some(ip) = payload.ip {
         let _ = sqlx::query("UPDATE bans SET ip = ? WHERE id = ?")
            .bind(ip).bind(id)
            .execute(&state.db).await;
    }
    if let Some(ban_type) = payload.ban_type {
         let _ = sqlx::query("UPDATE bans SET ban_type = ? WHERE id = ?")
            .bind(ban_type).bind(id)
            .execute(&state.db).await;
    }
    if let Some(reason) = payload.reason {
         let _ = sqlx::query("UPDATE bans SET reason = ? WHERE id = ?")
            .bind(reason).bind(id)
            .execute(&state.db).await;
    }
    if let Some(duration) = payload.duration {
         let expires_at = calculate_expires_at(&duration);
         let _ = sqlx::query("UPDATE bans SET duration = ?, expires_at = ? WHERE id = ?")
            .bind(duration).bind(expires_at).bind(id)
            .execute(&state.db).await;
    }

    let _ = log_admin_action(
        &state.db,
        &user.sub,
        "update_ban",
        &format!("BanID: {}", id),
        "Updated ban details"
    ).await;

    (StatusCode::OK, Json("Ban updated")).into_response()
}

pub async fn delete_ban(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    // Soft delete usually via status update, but API requested delete.
    let result = sqlx::query("DELETE FROM bans WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => {
            let _ = log_admin_action(
                &state.db,
                &user.sub,
                "delete_ban",
                &format!("BanID: {}", id),
                "Deleted ban"
            ).await;
            (StatusCode::OK, Json("Ban deleted")).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
