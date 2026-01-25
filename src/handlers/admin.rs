use axum::{
    extract::{Extension, Path, State},
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use std::sync::Arc;
use crate::AppState;
use crate::models::user::{Admin, CreateAdminRequest, UpdateAdminRequest};
use crate::handlers::auth::Claims;
use crate::utils::log_admin_action;
use bcrypt::{hash, DEFAULT_COST};

pub async fn list_admins(
    State(state): State<Arc<AppState>>,
) -> impl IntoResponse {
    let admins = sqlx::query_as::<_, Admin>("SELECT * FROM admins")
        .fetch_all(&state.db)
        .await;

    match admins {
        Ok(data) => (StatusCode::OK, Json(data)).into_response(),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn create_admin(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Json(payload): Json<CreateAdminRequest>,
) -> impl IntoResponse {
    let hashed = hash(payload.password, DEFAULT_COST).unwrap();

    let result = sqlx::query(
        "INSERT INTO admins (username, password, role, steam_id) VALUES (?, ?, ?, ?)"
    )
    .bind(&payload.username)
    .bind(hashed)
    .bind(&payload.role)
    .bind(&payload.steam_id)
    .execute(&state.db)
    .await;

    match result {
        Ok(_) => {
            let _ = log_admin_action(
                &state.db,
                &user.sub,
                "create_admin",
                &payload.username,
                &format!("Role: {}", payload.role)
            ).await;
            (StatusCode::CREATED, Json("Admin created")).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}

pub async fn update_admin(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Path(id): Path<i64>,
    Json(payload): Json<UpdateAdminRequest>,
) -> impl IntoResponse {
    if let Some(username) = payload.username {
        let _ = sqlx::query("UPDATE admins SET username = ? WHERE id = ?")
            .bind(username).bind(id)
            .execute(&state.db).await;
    }
    if let Some(password) = payload.password {
         let hashed = hash(password, DEFAULT_COST).unwrap();
         let _ = sqlx::query("UPDATE admins SET password = ? WHERE id = ?")
            .bind(hashed).bind(id)
            .execute(&state.db).await;
    }
    if let Some(role) = payload.role {
        let _ = sqlx::query("UPDATE admins SET role = ? WHERE id = ?")
            .bind(role).bind(id)
            .execute(&state.db).await;
    }
     if let Some(steam_id) = payload.steam_id {
        let _ = sqlx::query("UPDATE admins SET steam_id = ? WHERE id = ?")
            .bind(steam_id).bind(id)
            .execute(&state.db).await;
    }

    let _ = log_admin_action(
        &state.db,
        &user.sub,
        "update_admin",
        &format!("AdminID: {}", id),
        "Updated admin details"
    ).await;

    (StatusCode::OK, Json("Admin updated")).into_response()
}

pub async fn delete_admin(
    State(state): State<Arc<AppState>>,
    Extension(user): Extension<Claims>,
    Path(id): Path<i64>,
) -> impl IntoResponse {
    let result = sqlx::query("DELETE FROM admins WHERE id = ?")
        .bind(id)
        .execute(&state.db)
        .await;

    match result {
        Ok(_) => {
             let _ = log_admin_action(
                &state.db,
                &user.sub,
                "delete_admin",
                &format!("AdminID: {}", id),
                "Deleted admin"
            ).await;
            (StatusCode::OK, Json("Admin deleted")).into_response()
        },
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()).into_response(),
    }
}
