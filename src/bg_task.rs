use std::sync::Arc;
use crate::AppState;
use tokio::time::{interval, Duration};
use crate::models::server::Server;
use crate::models::ban::Ban;
use crate::utils::rcon::send_command;
use regex::Regex;
use chrono::Utc;

pub async fn start_background_task(state: Arc<AppState>) {
    tracing::info!("Background Task Started: Player IP Enforcement");
    let mut interval = interval(Duration::from_secs(60));

    loop {
        interval.tick().await;
        if let Err(e) = check_all_servers(&state).await {
            tracing::error!("Background Task Error: {}", e);
        }
    }
}

async fn check_all_servers(state: &Arc<AppState>) -> Result<(), Box<dyn std::error::Error>> {
    // 1. Get all Active IP Bans (to minimize DB hits in loop)
    // We only care about 'ip' bans that are active.
    let ip_bans = sqlx::query_as::<_, Ban>(
        "SELECT * FROM bans WHERE status = 'active' AND ban_type = 'ip'"
    )
    .fetch_all(&state.db)
    .await?;

    if ip_bans.is_empty() {
        return Ok(());
    }

    // 2. Get Servers
    let servers = sqlx::query_as::<_, Server>("SELECT * FROM servers")
        .fetch_all(&state.db)
        .await?;

    // 3. Check each server
    for server in servers {
        let address = format!("{}:{}", server.ip, server.port);
        let pwd = server.rcon_password.unwrap_or_default();

        match send_command(&address, &pwd, "status").await {
            Ok(output) => {
                // Parse Players
                // Use the regex we refined in get_server_players but simpler
                // Regex: # userid userid "name" steamid ... ip
                // Actually, let's just parse line by line more loosely to be safe
                for line in output.lines() {
                    let line = line.trim();
                    if !line.starts_with("#") { continue; }
                    
                    // Format: # <userid> <slot> "Name" <SteamID> ...
                    // Split by quote to isolate name
                    let parts: Vec<&str> = line.split('"').collect();
                    if parts.len() < 3 { continue; }
                    
                    // Parse UserID from Part 0: "# 123 "
                    let pre_name = parts[0].trim();
                    let pre_parts: Vec<&str> = pre_name.split_whitespace().collect();
                    // usually ["#", "123", "1"] or just ["#", "123"] depending on output format
                    // Let's assume the component after "#" is userid
                    let mut userid = "";
                    for (i, p) in pre_parts.iter().enumerate() {
                        if *p == "#" && i + 1 < pre_parts.len() {
                            userid = pre_parts[i+1];
                            break;
                        }
                    }
                    if userid.is_empty() { 
                         // Fallback: try last element
                         userid = pre_parts.last().unwrap_or(&"");
                    }
                    if userid == "#" { continue; } // Failed parsing

                    let player_name = parts[1]; // Real Name!

                    let after_name = parts[2].trim(); // STEAM_... ... IP:Port
                    let fields: Vec<&str> = after_name.split_whitespace().collect();
                    
                    if fields.len() < 2 { continue; }
                    
                    let steam_id = fields[0];
                    // The last field is usually IP:Port. 
                    let ip_port = fields.last().unwrap_or(&"");
                    let ip_only = ip_port.split(':').next().unwrap_or("");
                    
                    if ip_only.is_empty() || steam_id == "BOT" { continue; }

                    // CHECK: Is this IP in our ban list?
                    for ban in &ip_bans {
                        if ban.ip == ip_only {
                            let existing = sqlx::query(
                                "SELECT id FROM bans WHERE steam_id = ? AND status = 'active'"
                            )
                            .bind(steam_id)
                            .fetch_optional(&state.db)
                            .await?;

                            if existing.is_some() {
                                // Already banned - Just Kick
                                let _ = send_command(&address, &pwd, &format!("kickid {} \"Banned IP Detected\"", userid)).await;
                            } else {
                                // NEW CATCH!
                                tracing::info!("BG Task: Caught user bypassing IP Ban! IP: {}, SteamID: {}, Name: {}", ip_only, steam_id, player_name);
                                
                                let reason = "同IP关联封禁 (Detected online with Banned IP)";
                                let expires_at = ban.expires_at;

                                let _ = sqlx::query(
                                    "INSERT INTO bans (name, steam_id, ip, ban_type, reason, duration, admin_name, expires_at, created_at, status, server_id) VALUES (?, ?, ?, ?, ?, ?, ?, ?, NOW(), 'active', ?)"
                                )
                                .bind(player_name) // Use Real Name
                                .bind(steam_id)
                                .bind(ip_only)
                                .bind("account")
                                .bind(reason)
                                .bind(&ban.duration)
                                .bind("System (BG Monitor)")
                                .bind(expires_at)
                                .bind(server.id)
                                .execute(&state.db)
                                .await;

                                // Execution: Use sm_ban to BAN and KICK immediately on server side
                                // format: sm_ban #<userid> <minutes> "reason"
                                // This ensures they cannot reconnect even if DB check fails/timeouts
                                let duration_str = &ban.duration;
                                let _ = send_command(&address, &pwd, &format!("sm_ban #{} {} \"{}\"", userid, duration_str, reason)).await;
                            }
                        }
                    }
                }
            },
            Err(_) => continue, // Skip offline servers
        }
    }

    Ok(())
}
