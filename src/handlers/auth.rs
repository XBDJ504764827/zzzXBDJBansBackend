use axum::{
    extract::State,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use serde_json::json;
use std::sync::Arc;
use crate::AppState;
use crate::models::user::{LoginRequest, LoginResponse};
use bcrypt::verify;
use jsonwebtoken::{encode, Header, EncodingKey};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String, // username
    pub role: String,
    pub exp: usize,
}

pub async fn login(
    State(state): State<Arc<AppState>>,
    Json(payload): Json<LoginRequest>,
) -> impl IntoResponse {
    let row = sqlx::query_as::<_, crate::models::user::Admin>("SELECT * FROM admins WHERE username = ?")
        .bind(payload.username)
        .fetch_optional(&state.db)
        .await;

    match row {
        Ok(Some(user)) => {
            // Verify password
            // Note: In a real app we use bcrypt. 
            // For now, if string matches (for initial plaintext) OR bcrypt verify.
            // Our init migration inserts a bcrypt hash '$2y$10$...'
            // We should use bcrypt::verify.
            
            let valid = verify(&payload.password, &user.password).unwrap_or(false);
            // Fallback for simple testing if needed: || user.password == payload.password
            
            if valid {
                // Generate JWT
                let expiration = chrono::Utc::now()
                    .checked_add_signed(chrono::Duration::days(1))
                    .expect("valid timestamp")
                    .timestamp();

                let claims = Claims {
                    sub: user.username.clone(),
                    role: user.role.clone(),
                    exp: expiration as usize,
                };
                
                let secret = std::env::var("JWT_SECRET").unwrap_or_else(|_| "secret".to_string());
                let token = encode(&Header::default(), &claims, &EncodingKey::from_secret(secret.as_ref())).unwrap();

                // Log login? (Optional, requires log handler integration)
                
                return (StatusCode::OK, Json(json!({ "token": token, "user": { "username": user.username, "role": user.role } }))).into_response();
            }
        }
        Ok(None) => {}
        Err(_) => {}
    }

    (StatusCode::UNAUTHORIZED, Json(json!({ "error": "Invalid credentials" }))).into_response()
}

pub async fn logout() -> impl IntoResponse {
    // Stateless JWT, client just drops token. 
    // We can blacklist token in Redis if stricter.
    (StatusCode::OK, Json(json!({ "msg": "Logged out" })))
}

pub async fn me() -> impl IntoResponse {
    // Need middleware to extract claims. For now placeholder.
    (StatusCode::OK, Json(json!({ "msg": "Me" })))
}
