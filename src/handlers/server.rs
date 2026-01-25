use axum::{
    extract::{Path, State, Json, Extension},
    http::StatusCode,
    response::IntoResponse,
};
use std::sync::Arc;
use crate::AppState;
use crate::models::server::{
    ServerGroup, Server, GroupWithServers, 
    CreateGroupRequest, CreateServerRequest, UpdateServerRequest, CheckServerRequest
};
use crate::handlers::auth::Claims;
use crate::utils::log_admin_action; // Ensure this is accessible
use crate::utils::rcon::check_rcon;

// --- Groups ---

pub async fn list_server_groups(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    // Fetch all groups
    let groups = sqlx::query_as::<_, ServerGroup>("SELECT * FROM server_groups ORDER BY id ASC")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    // Fetch all servers
    let servers = sqlx::query_as::<_, Server>("SELECT * FROM servers")
        .fetch_all(&state.db)
        .await
        .unwrap_or_default();

    // Combine
    let mut result = Vec::new();
    for g in groups {
        let group_servers: Vec<Server> = servers.iter()
            .filter(|s| s.group_id == g.id)
            .map(|s| Server {
                id: s.id,
                group_id: s.group_id,
                name: s.name.clone(),
                ip: s.ip.clone(),
                port: s.port,
                rcon_password: s.rcon_password.clone(),
                created_at: s.created_at,
            })
            .collect();

        result.push(GroupWithServers {
            id: g.id,
            name: g.name,
            servers: group_servers,
        });
    }

    (StatusCode::OK, Json(result)).into_response()
}

pub async fn create_group(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Json(payload): Json<CreateGroupRequest>,
) -> impl IntoResponse {
    let result = sqlx::query("INSERT INTO server_groups (name) VALUES (?)")
        .bind(&payload.name)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => {
             let _ = log_admin_action(&state.db, &user.sub, "create_group", &payload.name, "Created server group").await;
            (StatusCode::CREATED, Json("Group created")).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn delete_group(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let result = sqlx::query("DELETE FROM server_groups WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => {
            let _ = log_admin_action(&state.db, &user.sub, "delete_group", &format!("ID: {}", id), "Deleted server group").await;
            (StatusCode::OK, Json("Group deleted")).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// --- Servers ---

pub async fn create_server(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Json(payload): Json<CreateServerRequest>,
) -> impl IntoResponse {
    let result = sqlx::query(
        "INSERT INTO servers (group_id, name, ip, port, rcon_password) VALUES (?, ?, ?, ?, ?)"
    )
    .bind(payload.group_id)
    .bind(&payload.name)
    .bind(&payload.ip)
    .bind(payload.port)
    .bind(&payload.rcon_password)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {
             let _ = log_admin_action(&state.db, &user.sub, "create_server", &payload.name, &format!("{}:{}", payload.ip, payload.port)).await;
            (StatusCode::CREATED, Json("Server created")).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn update_server(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateServerRequest>,
) -> impl IntoResponse {
    if let Some(name) = payload.name {
        let _ = sqlx::query("UPDATE servers SET name = ? WHERE id = ?").bind(name).bind(id).execute(&state.db).await;
    }
    if let Some(ip) = payload.ip {
        let _ = sqlx::query("UPDATE servers SET ip = ? WHERE id = ?").bind(ip).bind(id).execute(&state.db).await;
    }
    if let Some(port) = payload.port {
        let _ = sqlx::query("UPDATE servers SET port = ? WHERE id = ?").bind(port).bind(id).execute(&state.db).await;
    }
     if let Some(pwd) = payload.rcon_password {
        let _ = sqlx::query("UPDATE servers SET rcon_password = ? WHERE id = ?").bind(pwd).bind(id).execute(&state.db).await;
    }

     let _ = log_admin_action(&state.db, &user.sub, "update_server", &format!("ID: {}", id), "Updated server").await;

    (StatusCode::OK, Json("Server updated")).into_response()
}

pub async fn delete_server(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let result = sqlx::query("DELETE FROM servers WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => {
            let _ = log_admin_action(&state.db, &user.sub, "delete_server", &format!("ID: {}", id), "Deleted server").await;
            (StatusCode::OK, Json("Server deleted")).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

// --- Status Check ---

pub async fn check_server_status(
    Json(payload): Json<CheckServerRequest>,
) -> impl IntoResponse {
    let address = format!("{}:{}", payload.ip, payload.port);
    
    // Attempt RCON connection
    // Note: rcon crate usage depends on version. rcon 0.6.0 typically: 
    // Connection::builder().connect("address", "password").await
    
    let pwd = payload.rcon_password.unwrap_or_default();
    
    match check_rcon(&address, &pwd).await {
        Ok(_) => {
            (StatusCode::OK, Json("Connected successfully")).into_response()
        },
        Err(e) => {
            (StatusCode::BAD_REQUEST, Json(format!("Connection failed: {}", e))).into_response()
        }
    }
}
