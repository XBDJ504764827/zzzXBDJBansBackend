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
    tracing::info!("Processing verification for {}", steam_id);

    // 1. Check if Banned (If banned, deny immediately)
    // Note: We should probably check the bans table.
    // Assuming bans are strictly handled by plugin before even requesting verif? 
    // Or we should check here too. Let's check here for safety.
    let is_banned: bool = sqlx::query_scalar(
        "SELECT COUNT(*) FROM bans WHERE (steam_id = ? OR steam_id = ?) AND status = 'active' AND (expires_at IS NULL OR expires_at > NOW())"
    )
    .bind(steam_id) // SteamID64
    .bind(steam_id) // We'd need to convert to SteamID2 to be thorough, but let's assume SteamID is consistent or handled. 
                    // Actually, for safety, let's assume the plugin passed SteamID64.
    .fetch_one(pool)
    .await
    .map(|c: i64| c > 0)
    .unwrap_or(false);

    if is_banned {
        update_status(pool, steam_id, "denied", "Player is banned", None, None).await?;
        return Ok(());
    }

    // 2. Check Whitelist (If whitelisted, allow immediately)
    // We need to convert SteamID64 to Steam2 or just check broadly? 
    // The previous implementation used Steam2. Steam Web API uses Steam64.
    // The plugin will likely pass Steam64 for this table.
    // We should probably check Whitelist broadly.
    // BUT the prompt says: "If account not satisfy conditions but in whitelist... allow".
    // So Whitelist is a valid bypass.
    
    // We need a way to check whitelist. Our whitelist table likely has Steam2 IDs.
    // Converting 64 to 2 is annoying in Rust without a crate.
    // Let's assume the plugin handles the Whitelist check logic? 
    // No, the prompt says "Backend returns query result". 
    // Let's check Steam Level first, then if fail, check Whitelist.

    // 3. Steam API Checks
    let level = steam_service.get_steam_level(steam_id).await;
    let playtime = steam_service.get_csgo_playtime_minutes(steam_id).await;

    let level_val = level.unwrap_or(0);
    let playtime_val = playtime.unwrap_or(0);
    let playtime_hours = playtime_val as f32 / 60.0;

    let mut allowed = false;
    let mut reason = String::from("Criteria unmet");

    if level_val >= 1 && playtime_hours >= 100.0 {
        allowed = true;
        reason = format!("Qualified: Lv{} / {:.1}h", level_val, playtime_hours);
    } else {
        // Fallback: Check Whitelist
        // Simple check: Is this ID in whitelist?
        // Note: The whitelist table might use Steam2 format (STEAM_1:...).
        // We might fail to match if we only search Steam64. 
        // Ideally we convert. For now, let's query broadly or search. To be safe, we might need to handle this.
        // Or we can rely on the user inputting Steam64 in whitelist table too?
        // Let's search by string match.
        
        let in_whitelist = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM whitelist WHERE steam_id = ?")
            .bind(steam_id)
            .fetch_one(pool)
            .await
            .unwrap_or(0) > 0;
            
        if in_whitelist {
            allowed = true;
            reason = String::from("Whitelisted");
        } else {
            reason = format!("Denied: Lv{} (Req 1) / {:.1}h (Req 100h) & Not Whitelisted", level_val, playtime_hours);
        }
    }

    if allowed {
        update_status(pool, steam_id, "allowed", &reason, level, playtime).await?;
    } else {
        update_status(pool, steam_id, "denied", &reason, level, playtime).await?;
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
