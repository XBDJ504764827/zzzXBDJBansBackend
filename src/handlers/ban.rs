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
    
    let steam_id = params.steam_id.unwrap_or_default();
    let ip = params.ip.unwrap_or_default();
    
    tracing::info!("CHECK_BAN: Checking SteamID: {}, IP: {}", steam_id, ip);

    // 1. Check for DIRECT Account Ban (Matches SteamID)
    let account_ban = sqlx::query_as::<_, Ban>(
        "SELECT * FROM bans WHERE status = 'active' AND steam_id = ? LIMIT 1"
    )
    .bind(&steam_id)
    .fetch_optional(&state.db)
    .await;

    match account_ban {
        Ok(Some(mut b)) => {
            tracing::info!("CHECK_BAN: Found Account Ban: ID={}, Expires={:?}", b.id, b.expires_at);
            // Check expiration
            if let Some(expires_at) = b.expires_at {
                if Utc::now() > expires_at {
                    tracing::info!("CHECK_BAN: Account Ban Expired. Deactivating and falling through.");
                    let _ = sqlx::query("UPDATE bans SET status = 'expired' WHERE id = ?")
                        .bind(b.id).execute(&state.db).await;
                    // Expired - Do NOT return yet. Treat as not banned, proceed to check IP.
                } else {
                    return (StatusCode::OK, Json(b)).into_response();
                }
            } else {
                return (StatusCode::OK, Json(b)).into_response();
            }
        },
        Err(e) => {
             tracing::error!("CHECK_BAN: DB Error on Account Check: {}", e);
             return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response();
        },
        Ok(None) => {
            tracing::info!("CHECK_BAN: No Active Account Ban found.");
        }
    }

    // 2. Check for IP Ban (Matches IP AND ban_type = 'ip')
    tracing::info!("CHECK_BAN: Checking IP Ban for IP: {}", ip);
    let ip_ban = sqlx::query_as::<_, Ban>(
        "SELECT * FROM bans WHERE status = 'active' AND ip = ? AND ban_type = 'ip' LIMIT 1"
    )
    .bind(&ip)
    .fetch_optional(&state.db)
    .await;

    match ip_ban {
        Ok(Some(mut b)) => {
            tracing::info!("CHECK_BAN: Found IP Ban Match! Origin Ban ID: {}", b.id);
             // Check expiration for the IP ban
            if let Some(expires_at) = b.expires_at {
                if Utc::now() > expires_at {
                    tracing::info!("CHECK_BAN: IP Ban Expired.");
                    let _ = sqlx::query("UPDATE bans SET status = 'expired' WHERE id = ?")
                        .bind(b.id).execute(&state.db).await;
                    return (StatusCode::NOT_FOUND, Json("Not banned (Expired)")).into_response();
                }
            }

            // HIT! IP is banned, and user has no personal ban.
            tracing::info!("CHECK_BAN: IP Ban Hit for new identity! Triggering Auto-Ban. IP: {}, New SteamID: {}", ip, steam_id);

            // Create NEW Ban Record
            let reason = "同IP关联封禁 (Different account repeated IP login)".to_string();
            // Inherit expiration from the parent IP ban
            let expires_at = b.expires_at; 
            
            let insert_result = sqlx::query(
                "INSERT INTO bans (name, steam_id, ip, ban_type, reason, duration, admin_name, expires_at, created_at, status, server_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NOW(), 'active', ?)"
            )
            .bind("Auto-Banned") 
            .bind(&steam_id)
            .bind(&ip)
            .bind("account") 
            .bind(&reason)
            .bind(&b.duration) 
            .bind("System (IP Match)")
            .bind(expires_at)
            .bind(b.server_id)
            .execute(&state.db)
            .await;

            match insert_result {
                Ok(res) => {
                    let new_id = res.last_insert_id() as i64;
                    tracing::info!("CHECK_BAN: Auto-Ban Created Successfully. New ID: {}", new_id);
                    let new_ban = Ban {
                        id: new_id,
                        name: "Auto-Banned".to_string(),
                        steam_id: steam_id,
                        ip: ip,
                        ban_type: "account".to_string(),
                        reason: Some(reason),
                        duration: b.duration,
                        status: "active".to_string(),
                        admin_name: Some("System (IP Match)".to_string()),
                        created_at: Some(Utc::now()),
                        expires_at: expires_at,
                        server_id: b.server_id
                    };
                    return (StatusCode::OK, Json(new_ban)).into_response();
                },
                Err(e) => {
                    tracing::error!("CHECK_BAN: Failed to auto-create ban: {}", e);
                    // If insert fails, still return the IP ban so they are blocked
                    return (StatusCode::OK, Json(b)).into_response();
                }
            }
        },
        Ok(None) => {
            tracing::info!("CHECK_BAN: No IP Ban found. User is Clean.");
            return (StatusCode::NOT_FOUND, Json("Not banned")).into_response();
        },
        Err(e) => return (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
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
    tracing::info!("DELETE /api/bans/{} requested by user: {}, role: {}", id, user.sub, user.role);

    // 1. Permission Check
    if user.role != "super_admin" {
        tracing::warn!("Permission denied for user {}", user.sub);
        return (StatusCode::FORBIDDEN, Json("Only super admins can delete bans")).into_response();
    }

    // 2. Fetch Ban Details (for RCON unban)
    // Removed unwrap_or(None) to see actual error if mapping fails
    let ban_query = sqlx::query_as::<_, Ban>("SELECT * FROM bans WHERE id = ?")
        .bind(id)
        .fetch_optional(&state.db)
        .await;

    let ban = match ban_query {
        Ok(Some(b)) => b,
        Ok(None) => {
             tracing::warn!("Ban ID {} not found in DB", id);
             return (StatusCode::NOT_FOUND, "Ban not found").into_response();
        },
        Err(e) => {
             tracing::error!("DB Error fetching ban {}: {}", id, e);
             return (StatusCode::INTERNAL_SERVER_ERROR, format!("DB Error: {}", e)).into_response();
        }
    };

    tracing::info!("Found ban record: {} (Steam: {}, IP: {})", ban.name, ban.steam_id, ban.ip);

    // 3. Delete from DB first (for fast response)
    tracing::info!("Deleting ban ID {} from DB", id);
    let result = sqlx::query("DELETE FROM bans WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    match result {
        Ok(res) => {
            if res.rows_affected() == 0 {
                tracing::warn!("DELETE executed but 0 rows affected for ID {}", id);
            } else {
                // 4. Spawn RCON Unban task (Fire-and-forget)
                // Fetch servers inside the handler first to avoid lifetime issues or clone valid data
                let servers_result = sqlx::query_as::<_, crate::models::server::Server>("SELECT * FROM servers")
                    .fetch_all(&state.db)
                    .await;

                if let Ok(servers) = servers_result {
                    let ban_clone = ban.clone(); // Ban struct needs simple Clone derive or manual clone
                    // If Ban doesn't implement Clone, we might need to construct a lightweight struct or ensure it does.
                    // Assuming Ban implements Clone (it normally derives FromRow, Debug, Serialize, Deserialize - let's check or just clone fields)
                    // Let's manually reconstruct or assume Clone if easy. 
                    // Actually, let's just use the data we need: steam_id and ip.
                    let steam_id = ban.steam_id.clone();
                    let ip = ban.ip.clone();
                    let ban_name = ban.name.clone();

                    tokio::spawn(async move {
                        tracing::info!("Background task: Sending unban commands to {} servers for {}", servers.len(), ban_name);
                        use crate::utils::rcon::send_command;
                        
                        for server in servers {
                            let address = format!("{}:{}", server.ip, server.port);
                            let pwd = server.rcon_password.unwrap_or_default();
                            
                            // Unban SteamID
                            if !steam_id.is_empty() {
                                let cmd = format!("sm_unban \"{}\"", steam_id);
                                let _ = send_command(&address, &pwd, &cmd).await;
                            }
                            
                            // Unban IP
                            if !ip.is_empty() {
                                let cmd = format!("sm_unban \"{}\"", ip);
                                let _ = send_command(&address, &pwd, &cmd).await;
                            }
                        }
                        tracing::info!("Background task: Unban commands finished for {}", ban_name);
                    });
                }
            }

            let _ = log_admin_action(
                &state.db,
                &user.sub,
                "delete_ban",
                &format!("BanID: {}, Target: {} ({})", id, ban.name, ban.steam_id),
                "Deleted ban (Unban commands queued)"
            ).await;
            (StatusCode::OK, Json("Ban deleted, unban process started in background")).into_response()
        },
        Err(e) => {
            tracing::error!("Failed to delete ban from DB: {}", e);
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response()
        },
    }
}
