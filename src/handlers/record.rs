use axum::{
    extract::{State, Query},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use crate::AppState;
use crate::models::record::{PlayerRecord, CreateRecordRequest};
use serde::Deserialize;

#[derive(Deserialize)]
pub struct RecordFilter {
    search: Option<String>,
}

pub async fn list_records(
    State(state): State<Arc<AppState>>,
    Query(params): Query<RecordFilter>,
) -> impl IntoResponse {
    let mut query = "SELECT * FROM player_records".to_string();
    
    // Simple search implementation
    // Using string formatting for simplicity in this demo, but should use binders for security
    // sqlx doesn't support dynamic binders cleanly without QueryBuilder.
    // For now, fetch latest 100.
    // If search is present, we filter.
    
    if let Some(s) = params.search {
        // Warning: SQL Injection risk if direct interpolation. 
        // DO NOT DO: query = format!("... LIKE '%{}%'", s);
        // We will use sqlx::query_as with QueryBuilder or just a fixed query for this demo.
        // Let's use a fixed query for "search anything"
        let pattern = format!("%{}%", s);
        let records = sqlx::query_as::<_, PlayerRecord>(
            "SELECT * FROM player_records WHERE player_name LIKE ? OR steam_id LIKE ? OR player_ip LIKE ? ORDER BY connect_time DESC LIMIT 100"
        )
        .bind(&pattern)
        .bind(&pattern)
        .bind(&pattern)
        .fetch_all(&state.db)
        .await;
        
        return match records {
            Ok(data) => (StatusCode::OK, Json(data)).into_response(),
            Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
        };
    }

    query.push_str(" ORDER BY connect_time DESC LIMIT 100");
    
    let records = sqlx::query_as::<_, PlayerRecord>(&query)
        .fetch_all(&state.db)
        .await;

    match records {
        Ok(data) => (StatusCode::OK, Json(data)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn create_record(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<CreateRecordRequest>,
) -> impl IntoResponse {
    let result = sqlx::query(
        "INSERT INTO player_records (player_name, steam_id, player_ip, server_name, server_address) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(payload.player_name)
    .bind(payload.steam_id)
    .bind(payload.player_ip)
    .bind(payload.server_name)
    .bind(payload.server_address)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => (StatusCode::CREATED, Json("Record created")).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
