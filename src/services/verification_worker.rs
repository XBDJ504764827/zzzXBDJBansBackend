use std::time::Duration;
use sqlx::{MySqlPool, Row};
use crate::services::steam_api::SteamService;
use crate::models::whitelist::Whitelist;

pub async fn start_verification_worker(pool: MySqlPool) {
    let steam_service = SteamService::new();
    tracing::info!("Verification Worker started.");

    loop {
        // Poll pending requests
        let pending = sqlx::query("SELECT steam_id FROM player_verifications WHERE status = 'pending' LIMIT 10")
            .fetch_all(&pool)
            .await;

        if let Ok(rows) = pending {
            for row in rows {
                let steam_id: String = row.get("steam_id");
                
                // Process each user
                match process_user(&pool, &steam_service, &steam_id).await {
                    Ok(_) => {},
                    Err(e) => tracing::error!("Error processing verif for {}: {:?}", steam_id, e),
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn process_user(pool: &MySqlPool, steam_service: &SteamService, steam_id: &str) -> anyhow::Result<()> {


    // Special Case: Bots
    if steam_id.eq_ignore_ascii_case("BOT") {
        update_status(pool, steam_id, "allowed", "机器人", None, None).await?;
        return Ok(());
    }

    // 0. Resolve SteamID
    let resolved_id = steam_service.resolve_steam_id(steam_id).await.unwrap_or_else(|| steam_id.to_string());
    // Try to convert to SteamID2 for DB matching
    let steam_id_2 = steam_service.id64_to_id2(&resolved_id).unwrap_or_else(|| resolved_id.clone());
    


    // 1. Check if Banned
    // Check against Resolved(64), Input, AND SteamID2
    let is_banned: bool = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bans WHERE (steam_id = ? OR steam_id = ? OR steam_id = ?) AND status = 'active' AND (expires_at IS NULL OR expires_at > NOW())"
    )
    .bind(&resolved_id)
    .bind(steam_id)
    .bind(&steam_id_2)
    .fetch_one(pool)
    .await
    .map(|c: i64| c > 0)
    .unwrap_or(false);

    if is_banned {
        update_status(pool, steam_id, "denied", "依然在封禁中", None, None).await?;
        return Ok(());
    }

    // 2. Fetch All Metrics
    let gokz_rating_opt = steam_service.get_gokz_rating(&resolved_id).await;
    let level_opt = steam_service.get_steam_level(&resolved_id).await;
    let playtime_opt = steam_service.get_csgo_playtime_minutes(&resolved_id).await;

    let gokz_rating = gokz_rating_opt.unwrap_or(0.0);
    let level_val = level_opt.unwrap_or(0);
    let playtime_val = playtime_opt.unwrap_or(0);
    let playtime_hours = playtime_val as f32 / 60.0;



    let mut allowed = false;
    let mut reason = String::from("未满足条件");

    // 3. Strict Criteria Check
    // Requirement: Rating >= 4 AND Level >= 1 AND Playtime >= 100h
    if gokz_rating >= 4.0 && level_val >= 1 && playtime_hours >= 100.0 {
        allowed = true;
        reason = format!("验证通过: Rating {:.2} / 等级 {} / 时长 {:.1}h", gokz_rating, level_val, playtime_hours);
    } else {
        // 4. Fallback: Whitelist Check
        let in_whitelist = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM whitelist WHERE steam_id = ? OR steam_id = ? OR steam_id = ?")
            .bind(&resolved_id)
            .bind(steam_id)
            .bind(&steam_id_2)
            .fetch_one(pool)
            .await
            .unwrap_or(0) > 0;
            
        if in_whitelist {
            allowed = true;
            reason = String::from("白名单用户");
        } else {
            // Detailed failure reason in Chinese
            reason = format!("验证失败: Rating {:.2}(需>=4) / 等级 {}(需>=1) / 时长 {:.1}h(需>=100h) 且不在白名单", gokz_rating, level_val, playtime_hours);
        }
    }

    if allowed {
        update_status(pool, steam_id, "allowed", &reason, Some(level_val), Some(playtime_val)).await?;
    } else {
        update_status(pool, steam_id, "denied", &reason, Some(level_val), Some(playtime_val)).await?;
    }

    Ok(())
}

async fn update_status(pool: &MySqlPool, steam_id: &str, status: &str, reason: &str, level: Option<i32>, playtime: Option<i32>) -> anyhow::Result<()> {
    sqlx::query("UPDATE player_verifications SET status = ?, reason = ?, steam_level = ?, playtime_minutes = ?, updated_at = NOW() WHERE steam_id = ?")
        .bind(status)
        .bind(reason)
        .bind(level)
        .bind(playtime)
        .bind(steam_id)
        .execute(pool)
        .await?;
    Ok(())
}
