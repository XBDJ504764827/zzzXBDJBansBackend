use axum::{
    extract::{Path, State},
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use crate::{AppState, models::whitelist::{Whitelist, CreateWhitelistRequest}};
use serde_json::json;

// ... imports
use crate::services::steam_api::SteamService;

pub async fn list_whitelist(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let whitelist = sqlx::query_as::<_, Whitelist>("SELECT * FROM whitelist ORDER BY created_at DESC")
        .fetch_all(&state.db)
        .await
        .unwrap_or_else(|e| {
            tracing::error!("Failed to fetch whitelist: {:?}", e);
            vec![]
        });

    Json(whitelist)
}

pub async fn create_whitelist(
    State(state): State<Arc<AppState>>,
    Json(mut payload): Json<CreateWhitelistRequest>,
) -> impl IntoResponse {
    // CONVERSION: Ensure SteamID is SteamID2
    let steam_service = SteamService::new();
    if let Some(id64) = steam_service.resolve_steam_id(&payload.steam_id).await {
         if let Some(id2) = steam_service.id64_to_id2(&id64) {

             payload.steam_id = id2;
         } else {
             tracing::warn!("CREATE_WHITELIST: Failed to convert ID64 {} to ID2", id64);
         }
    } else {
         tracing::warn!("CREATE_WHITELIST: Failed to resolve SteamID {}", payload.steam_id);
    }

    let result = sqlx::query(
        "INSERT INTO whitelist (steam_id, name, status) VALUES (?, ?, 'approved')",
    )
    .bind(&payload.steam_id)
    .bind(&payload.name)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => (axum::http::StatusCode::CREATED, Json(json!({ "message": "Whitelist added" }))),
        Err(e) => {
            tracing::error!("Failed to add whitelist: {:?}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Failed to add whitelist" })))
        }
    }
}

pub async fn delete_whitelist(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let result = sqlx::query("DELETE FROM whitelist WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => (axum::http::StatusCode::OK, Json(json!({ "message": "Whitelist deleted" }))),
        Err(e) => {
            tracing::error!("Failed to delete whitelist: {:?}", e);
            (axum::http::StatusCode::INTERNAL_SERVER_ERROR, Json(json!({ "error": "Failed to delete whitelist" })))
        }
    }
}
