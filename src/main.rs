use axum::{
    routing::get,
    Router,
};
use dotenvy::dotenv;
use std::net::SocketAddr;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tower_http::cors::CorsLayer;

mod db;
mod handlers;
mod models;

// Application State
pub struct AppState {
    pub db: sqlx::MySqlPool,
    // pub redis: redis::Client, // TBD
}

#[tokio::main]
async fn main() {
    dotenv().ok();
    tracing_subscriber::fmt::init();

    let pool = db::establish_connection().await;

    // Run migrations
    sqlx::migrate!("./migrations")
        .run(&pool)
        .await
        .expect("Failed to run migrations");

    ensure_super_admin(&pool).await;

    let state = Arc::new(AppState { db: pool });

    let app = Router::new()
        .route("/", get(root))
        // Auth
        .route("/api/auth/login", axum::routing::post(handlers::auth::login))
        .route("/api/auth/me", get(handlers::auth::me))
        .route("/api/auth/logout", axum::routing::post(handlers::auth::logout))
        // Admins
        .route("/api/admins", get(handlers::admin::list_admins).post(handlers::admin::create_admin))
        .route("/api/admins/:id", axum::routing::put(handlers::admin::update_admin).delete(handlers::admin::delete_admin))
        // Bans
        .route("/api/bans", get(handlers::ban::list_bans).post(handlers::ban::create_ban))
        .route("/api/bans/:id", axum::routing::put(handlers::ban::update_ban).delete(handlers::ban::delete_ban))
        .route("/api/check_ban", get(handlers::ban::check_ban))
        // Logs
        .route("/api/logs", get(handlers::log::list_logs).post(handlers::log::create_log))
        // Records
        .route("/api/records", get(handlers::record::list_records).post(handlers::record::create_record))
        
        .layer(TraceLayer::new_for_http())
        .layer(CorsLayer::permissive())
        .with_state(state);

    let host = std::env::var("SERVER_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let port = std::env::var("SERVER_PORT").unwrap_or_else(|_| "3000".to_string());
    let addr = format!("{}:{}", host, port).parse::<SocketAddr>().expect("Invalid address");

    tracing::info!("listening on {}", addr);
    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn root() -> &'static str {
    "zzzXBDJBans Backend API"
}

async fn ensure_super_admin(pool: &sqlx::MySqlPool) {
    let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM admins")
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    if count == 0 {
        tracing::info!("No admins found. Creating default super_admin.");
        let username = "admin";
        let password = "123"; 
        let hashed = bcrypt::hash(password, bcrypt::DEFAULT_COST).expect("Failed to hash password");
        
        let _ = sqlx::query(
            "INSERT INTO admins (username, password, role) VALUES (?, ?, 'super_admin')"
        )
        .bind(username)
        .bind(hashed)
        .execute(pool)
        .await
        .expect("Failed to create default admin");
        
        tracing::info!("Default admin created: user='admin', pass='123'");
    } else {
        // Fix for potential bad migration data: if admin exists with placeholder password, reset it.
        let placeholder = "$2y$10$YourHashedPasswordHereOrImplementRegister";
        let row: Option<(i64, String)> = sqlx::query_as("SELECT id, password FROM admins WHERE username = 'admin'")
             .fetch_optional(pool).await.unwrap_or(None);
             
        if let Some((id, pass)) = row {
            if pass == placeholder {
                 tracing::info!("Found placeholder password for 'admin'. Resetting to default.");
                 let hashed = bcrypt::hash("123", bcrypt::DEFAULT_COST).unwrap();
                 let _ = sqlx::query("UPDATE admins SET password = ? WHERE id = ?")
                    .bind(hashed)
                    .bind(id)
                    .execute(pool).await;
                 tracing::info!("Admin password reset to '123'");
            }
        }
    }
}
