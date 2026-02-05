use std::time::Duration;
use sqlx::{MySqlPool, Row};
use crate::services::steam_api::SteamService;

pub async fn start_verification_worker(pool: MySqlPool) {
    let steam_service = SteamService::new();
    tracing::info!("Verification Worker started.");

    loop {
        // 1. Priority: Manual Verifications
        if let Ok(rows) = sqlx::query("SELECT steam_id FROM player_verifications WHERE status = 'pending' LIMIT 10")
            .fetch_all(&pool)
            .await
        {
            for row in rows {
                let steam_id: String = row.get("steam_id");
                if let Err(e) = fetch_and_save_data(&pool, &steam_service, &steam_id, "player_verifications").await {
                    tracing::error!("Verification error for {}: {:?}", steam_id, e);
                }
            }
        }

        // 2. Secondary: Player Cache
        if let Ok(rows) = sqlx::query("SELECT steam_id FROM player_cache WHERE status = 'pending' LIMIT 10")
            .fetch_all(&pool)
            .await
        {
            for row in rows {
                let steam_id: String = row.get("steam_id");
                if let Err(e) = fetch_and_save_data(&pool, &steam_service, &steam_id, "player_cache").await {
                    tracing::error!("Verification error for {}: {:?}", steam_id, e);
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

/// 从 API 获取数据并保存到数据库
async fn fetch_and_save_data(
    pool: &MySqlPool, 
    steam_service: &SteamService, 
    steam_id: &str, 
    table: &str
) -> anyhow::Result<()> {
    if steam_id.eq_ignore_ascii_case("BOT") {
        update_data(pool, table, steam_id, Some(0), Some(0), Some(0.0)).await?;
        return Ok(());
    }

    let resolved_id = steam_service.resolve_steam_id(steam_id).await
        .unwrap_or_else(|| steam_id.to_string());

    let gokz_rating = steam_service.get_gokz_rating(&resolved_id).await.unwrap_or(0.0);
    let level = steam_service.get_steam_level(&resolved_id).await.unwrap_or(0);
    let playtime = steam_service.get_csgo_playtime_minutes(&resolved_id).await.unwrap_or(0);

    update_data(pool, table, steam_id, Some(level), Some(playtime), Some(gokz_rating)).await?;
    Ok(())
}

/// 更新数据并将状态设为 verified
async fn update_data(
    pool: &MySqlPool, 
    table: &str, 
    steam_id: &str, 
    level: Option<i32>, 
    playtime: Option<i32>, 
    gokz_rating: Option<f64>
) -> anyhow::Result<()> {
    let query = format!(
        "UPDATE {} SET status = 'verified', steam_level = ?, playtime_minutes = ?, gokz_rating = ?, updated_at = NOW() WHERE steam_id = ?", 
        table
    );
    sqlx::query(&query)
        .bind(level)
        .bind(playtime)
        .bind(gokz_rating)
        .bind(steam_id)
        .execute(pool)
        .await?;
    Ok(())
}
