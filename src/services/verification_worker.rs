use std::time::Duration;
use sqlx::{MySqlPool, Row};
use crate::services::steam_api::SteamService;
use crate::models::whitelist::Whitelist;
use crate::utils::log_admin_action;
use redis::AsyncCommands; // Import for set_ex, get

pub async fn start_verification_worker(pool: MySqlPool, redis_client: redis::Client) {
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
                match process_user(&pool, &redis_client, &steam_service, &steam_id).await {
                    Ok(_) => {},
                    Err(e) => tracing::error!("Error processing verif for {}: {:?}", steam_id, e),
                }
            }
        }

        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

#[derive(serde::Serialize, serde::Deserialize)]
struct RedisCacheData {
    level: i32,
    playtime: i32,
    rating: f64,
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

async fn process_user(pool: &MySqlPool, redis_client: &redis::Client, steam_service: &SteamService, steam_id: &str) -> anyhow::Result<()> {
    // Special Case: Bots
    if steam_id.eq_ignore_ascii_case("BOT") {
        let _ = log_admin_action(pool, "System", "player_verification", steam_id, "Allowed: Bot").await;
        update_status(pool, steam_id, "allowed", "Bot", None, None).await?;
        return Ok(());
    }

    // 0. Resolve SteamID
    let resolved_id = steam_service.resolve_steam_id(steam_id).await.unwrap_or_else(|| steam_id.to_string());
    let steam_id_2 = steam_service.id64_to_id2(&resolved_id).unwrap_or_else(|| resolved_id.clone());

    // 1. Check if Banned
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
        let _ = log_admin_action(pool, "System", "player_verification", steam_id, "Denied: Account Banned").await;
        update_status(pool, steam_id, "denied", "Account Banned", None, None).await?;
        return Ok(());
    }

    // 2. REDIS CACHE CHECK (24h)
    // Key format: "verif:{steam_id}"
    let redis_key = format!("verif:{}", steam_id);
    let mut con = redis_client.get_multiplexed_async_connection().await?;
    
    let cached_json: Option<String> = con.get(&redis_key).await.unwrap_or(None);

    let mut level_val = 0;
    let mut playtime_val = 0;
    let mut gokz_rating = 0.0;
    
    let mut use_api = true;

    if let Some(json_str) = cached_json {
        if let Ok(data) = serde_json::from_str::<RedisCacheData>(&json_str) {
            tracing::info!("Hit Redis Cache for {}: Level={}, Time={}, Rating={}", steam_id, data.level, data.playtime, data.rating);
            level_val = data.level;
            playtime_val = data.playtime;
            gokz_rating = data.rating;
            use_api = false; 
        }
    }

    // 3. MySQL Safe-Check (Only if Redis missing)
    // We still check MySQL for recent "reason" parsing if needed (network fallback), 
    // BUT user priority is: Redis Valid = Skip All. Redis Invalid/Missing = Re-verify.
    // However, if we re-verify and API fails, we STILL need fallback to MySQL old data if it exists?
    // User said: "If query player qualification not passed, then... subsequent ... check again".
    // "Only valid players are cached".
    
    // So if API fails (network issue), we should probably fallback to old MySQL data to avoid kicking VALID player who just had expired redis cache?
    // Let's keep the MySQL fallback logic for SAFETY, but only trigger it if use_api is true.
    
    if use_api {
          // Fetch metrics via API
        let gokz_rating_opt = steam_service.get_gokz_rating(&resolved_id).await;
        let level_opt = steam_service.get_steam_level(&resolved_id).await;
        let playtime_opt = steam_service.get_csgo_playtime_minutes(&resolved_id).await;
        
        gokz_rating = gokz_rating_opt.unwrap_or(0.0);
        level_val = level_opt.unwrap_or(0);
        playtime_val = playtime_opt.unwrap_or(0);

        // Fallback Logic: If API failed (values are 0/None), try to fetch from MySQL old records
        // This is crucial for "network problem" resilience.
        if gokz_rating == 0.0 || level_val == 0 || playtime_val == 0 {
             #[derive(sqlx::FromRow)]
            struct OldData {
                steam_level: Option<i32>,
                playtime_minutes: Option<i32>,
                reason: Option<String>,
            }
            let old: Option<OldData> = sqlx::query_as("SELECT steam_level, playtime_minutes, reason FROM player_verifications WHERE steam_id = ?")
                .bind(steam_id).fetch_optional(pool).await.unwrap_or(None);

            if let Some(o) = old {
                if level_val == 0 { level_val = o.steam_level.unwrap_or(0); }
                if playtime_val == 0 { playtime_val = o.playtime_minutes.unwrap_or(0); }
                if gokz_rating == 0.0 {
                    // Try parse rating
                    if let Some(r_str) = &o.reason {
                        if let Some(caps) = regex::Regex::new(r"Rating\s+([\d\.]+)").unwrap().captures(r_str) {
                             if let Ok(r) = caps[1].parse::<f64>() { gokz_rating = r; }
                        }
                    }
                }
            }
        }
    }

    let playtime_hours = playtime_val as f32 / 60.0;
    
    let mut allowed = false;
    let mut reason = String::from("Requirements not met");

    // 4. Strict Criteria Check
    if gokz_rating >= 2.5 && level_val >= 1 && playtime_hours >= 100.0 {
        allowed = true;
        reason = format!("Verified: Rating {:.2} / Level {} / Hours {:.1}h", gokz_rating, level_val, playtime_hours);
    } else {
        // 5. Fallback: Whitelist Check
        let in_whitelist = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM whitelist WHERE steam_id = ? OR steam_id = ? OR steam_id = ?")
            .bind(&resolved_id)
            .bind(steam_id)
            .bind(&steam_id_2)
            .fetch_one(pool)
            .await
            .unwrap_or(0) > 0;
            
        if in_whitelist {
            allowed = true;
            reason = String::from("Whitelisted");
        } else {
            reason = format!("Verify Failed: Rating {:.2}(Req>=4) / Level {}(Req>=1) / Hours {:.1}h(Req>=100h) & Not Whitelisted", gokz_rating, level_val, playtime_hours);
        }
    }

    if allowed {
        // CACHE SUCCESS IN REDIS (24 HOURS)
        // We only cache if they are verified.
        // User Requirement: "if not passed ... not stored in cache".
        // "Until passed ... write to cache".
        if use_api { // Only write if we just fetched/verified it. If we used cache, no need to re-set (or maybe refresh TTL?)
            // Let's refresh TTL or set it.
            let cache_data = RedisCacheData {
                level: level_val,
                playtime: playtime_val,
                rating: gokz_rating,
            };
            if let Ok(json) = serde_json::to_string(&cache_data) {
                let _: () = con.set_ex(&redis_key, json, 24 * 60 * 60).await.unwrap_or(());
                tracing::info!("Cached verified status for {} in Redis (24h)", steam_id);
            }
        }
        
        // Log Success
        let _ = log_admin_action(
            pool, 
            "System", 
            "player_verification", 
            steam_id, 
            &format!("Allowed: {}", reason)
        ).await;

        update_status(pool, steam_id, "allowed", &reason, Some(level_val), Some(playtime_val)).await?;
    } else {
        // Verification Failed
        // DO NOT CACHE in Redis.
        let _ = log_admin_action(
            pool, 
            "System", 
            "player_verification", 
            steam_id, 
            &format!("Denied: {}", reason)
        ).await;
        update_status(pool, steam_id, "denied", &reason, Some(level_val), Some(playtime_val)).await?;
    }

    Ok(())
}
