use serde::Deserialize;

const STEAM_API_KEY: &str = "2F4169922F55822ED36571D3B946E457";

#[derive(Debug, Deserialize)]
struct SteamLevelResponse {
    response: SteamLevelData,
}

#[derive(Debug, Deserialize)]
struct SteamLevelData {
    player_level: Option<i32>,
}

#[derive(Debug, Deserialize)]
struct OwnedGamesResponse {
    response: OwnedGamesData,
}

#[derive(Debug, Deserialize)]
struct OwnedGamesData {
    games: Option<Vec<SteamGame>>,
}

#[derive(Debug, Deserialize)]
struct SteamGame {
    appid: u32,
    playtime_forever: i32, // Minutes
}

pub struct SteamService {
    client: reqwest::Client,
}

impl SteamService {
    pub fn new() -> Self {
        Self {
            client: reqwest::Client::new(),
        }
    }

    pub async fn get_steam_level(&self, steam_id_64: &str) -> Option<i32> {
        let url = format!(
            "https://api.steampowered.com/IPlayerService/GetSteamLevel/v1/?key={}&steamid={}",
            STEAM_API_KEY, steam_id_64
        );

        match self.client.get(&url).send().await {
            Ok(resp) => {
                if let Ok(data) = resp.json::<SteamLevelResponse>().await {
                    return data.response.player_level;
                }
            }
            Err(e) => tracing::error!("Steam API Level Error: {}", e),
        }
        None
    }

    pub async fn get_csgo_playtime_minutes(&self, steam_id_64: &str) -> Option<i32> {
        // CS:GO AppID = 730
        let url = format!(
            "https://api.steampowered.com/IPlayerService/GetOwnedGames/v0001/?key={}&steamid={}&format=json",
            STEAM_API_KEY, steam_id_64
        );

        match self.client.get(&url).send().await {
            Ok(resp) => {
                if let Ok(data) = resp.json::<OwnedGamesResponse>().await {
                    if let Some(games) = data.response.games {
                        for game in games {
                            if game.appid == 730 {
                                return Some(game.playtime_forever);
                            }
                        }
                    }
                    // Games list returned but CSGO not found -> 0 minutes
                    return Some(0); 
                }
            }
            Err(e) => tracing::error!("Steam API Games Error: {}", e),
        }
        None
    }
}
