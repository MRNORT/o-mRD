use anyhow::{anyhow, Result};
use chrono::{DateTime, Utc};
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// osu! API v2 client
#[derive(Clone)]
pub struct OsuClient {
    client: Client,
    token: Arc<RwLock<Option<TokenInfo>>>,
    client_id: String,
    client_secret: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
struct TokenResponse {
    access_token: String,
    expires_in: u64,
    token_type: String,
}

#[derive(Debug, Clone)]
struct TokenInfo {
    access_token: String,
    expires_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Beatmapset {
    pub id: u64,
    pub title: String,
    pub title_unicode: String,
    pub artist: String,
    pub artist_unicode: String,
    pub creator: String,
    pub source: String,
    pub status: String,
    pub ranked_date: Option<DateTime<Utc>>,
    pub submitted_date: Option<DateTime<Utc>>,
    pub preview_url: Option<String>,
    pub beatmaps: Option<Vec<Beatmap>>,
    pub covers: Option<BeatmapCovers>,
    pub play_count: u64,
    pub favourite_count: u64,
    pub bpm: Option<f64>,
    pub tags: String,
    pub video: Option<bool>,
}

impl Beatmapset {
    pub fn key_counts(&self) -> Vec<u32> {
        if let Some(maps) = &self.beatmaps {
            let mut keys: Vec<u32> = maps
                .iter()
                .filter(|m| m.mode == "mania")
                .filter_map(|m| m.cs.map(|cs| cs as u32))
                .collect();
            keys.sort();
            keys.dedup();
            keys
        } else {
            vec![]
        }
    }

    pub fn max_sr(&self) -> f64 {
        self.beatmaps
            .as_ref()
            .map(|maps| {
                maps.iter()
                    .filter(|m| m.mode == "mania")
                    .map(|m| m.difficulty_rating)
                    .fold(0.0_f64, f64::max)
            })
            .unwrap_or(0.0)
    }

    pub fn min_sr(&self) -> f64 {
        self.beatmaps
            .as_ref()
            .map(|maps| {
                maps.iter()
                    .filter(|m| m.mode == "mania")
                    .map(|m| m.difficulty_rating)
                    .fold(f64::INFINITY, f64::min)
            })
            .unwrap_or(0.0)
    }

    #[allow(dead_code)]
    pub fn diff_count(&self) -> usize {
        self.beatmaps
            .as_ref()
            .map(|maps| maps.iter().filter(|m| m.mode == "mania").count())
            .unwrap_or(0)
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct Beatmap {
    pub id: u64,
    pub beatmapset_id: u64,
    pub version: String,
    pub mode: String,
    pub difficulty_rating: f64,
    pub cs: Option<f64>, // key count in mania
    pub bpm: Option<f64>,
    pub total_length: u32,
    pub status: String,
    pub ranked: Option<i32>,
    pub playcount: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Default)]
#[serde(default)]
pub struct BeatmapCovers {
    pub cover: String,
    #[serde(rename = "cover@2x")]
    pub cover_2x: String,
    pub card: String,
    #[serde(rename = "card@2x")]
    pub card_2x: String,
    pub list: String,
    #[serde(rename = "list@2x")]
    pub list_2x: String,
    #[serde(rename = "slimcover")]
    pub slimcover: String,
    #[serde(rename = "slimcover@2x")]
    pub slimcover_2x: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone, Deserialize)]
pub struct SearchResponse {
    pub beatmapsets: Vec<Beatmapset>,
    pub cursor_string: Option<String>,
    pub total: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchFilters {
    pub key_counts: Vec<u32>, // empty = all
    pub min_sr: Option<f64>,
    pub max_sr: Option<f64>,
    pub min_bpm: Option<f64>,
    pub max_bpm: Option<f64>,
    pub query: String,
    pub sort: SortBy,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub enum SortBy {
    #[default]
    RankedDesc,
    RankedAsc,
    Plays,
    Favourites,
    Difficulty,
    Title,
}

impl SortBy {
    pub fn as_api_param(&self) -> &str {
        match self {
            SortBy::RankedDesc => "ranked_desc",
            SortBy::RankedAsc => "ranked_asc",
            SortBy::Plays => "plays_desc",
            SortBy::Favourites => "favourites_desc",
            SortBy::Difficulty => "difficulty_desc",
            SortBy::Title => "title_asc",
        }
    }

    pub fn display(&self) -> &str {
        match self {
            SortBy::RankedDesc => "Newest Ranked",
            SortBy::RankedAsc => "Oldest Ranked",
            SortBy::Plays => "Most Played",
            SortBy::Favourites => "Most Favourited",
            SortBy::Difficulty => "Hardest",
            SortBy::Title => "Title A-Z",
        }
    }
}

impl OsuClient {
    pub fn new(client_id: String, client_secret: String) -> Self {
        let client = Client::builder()
            .user_agent("osu-mania-dl/0.1")
            .timeout(std::time::Duration::from_secs(30))
            .build()
            .expect("Failed to build HTTP client");

        Self {
            client,
            token: Arc::new(RwLock::new(None)),
            client_id,
            client_secret,
        }
    }

    pub async fn authenticate(&self) -> Result<()> {
        let resp = self
            .client
            .post("https://osu.ppy.sh/oauth/token")
            .json(&HashMap::from([
                ("client_id", self.client_id.as_str()),
                ("client_secret", self.client_secret.as_str()),
                ("grant_type", "client_credentials"),
                ("scope", "public"),
            ]))
            .send()
            .await?;

        if !resp.status().is_success() {
            let text = resp.text().await.unwrap_or_default();
            return Err(anyhow!("Auth failed: {}", text));
        }

        let token_resp: TokenResponse = resp.json().await?;
        let expires_at = Utc::now() + chrono::Duration::seconds(token_resp.expires_in as i64 - 60);

        let mut token = self.token.write().await;
        *token = Some(TokenInfo {
            access_token: token_resp.access_token,
            expires_at,
        });

        Ok(())
    }

    pub async fn ensure_token(&self) -> Result<String> {
        {
            let token = self.token.read().await;
            if let Some(ref t) = *token {
                if t.expires_at > Utc::now() {
                    return Ok(t.access_token.clone());
                }
            }
        }
        self.authenticate().await?;
        let token = self.token.read().await;
        Ok(token
            .as_ref()
            .ok_or_else(|| anyhow!("No token after auth"))?
            .access_token
            .clone())
    }

    pub async fn search_ranked_mania(
        &self,
        filters: &SearchFilters,
        cursor_string: Option<&str>,
    ) -> Result<SearchResponse> {
        let use_nerinyan = self.client_id.trim().is_empty() || self.client_secret.trim().is_empty();

        let mut params: Vec<(&str, String)> = vec![
            ("m", "3".to_string()),
            ("s", "ranked".to_string()),
            ("sort", filters.sort.as_api_param().to_string()),
        ];

        if !filters.query.is_empty() {
            params.push(("q", filters.query.clone()));
        }

        let mut search_resp: SearchResponse = if use_nerinyan {
            let req = self.client.get("https://api.nerinyan.moe/search").query(&params).send().await?;
            if !req.status().is_success() {
                let status = req.status();
                let text = req.text().await.unwrap_or_default();
                return Err(anyhow!("Nerinyan API error {}: {}", status, text));
            }
            let beatmapsets: Vec<Beatmapset> = req.json().await?;
            SearchResponse {
                beatmapsets,
                cursor_string: None,
                total: None,
                error: None,
            }
        } else {
            if let Some(cursor) = cursor_string {
                params.push(("cursor_string", cursor.to_string()));
            }
            let token = self.ensure_token().await?;
            let resp = self
                .client
                .get("https://osu.ppy.sh/api/v2/beatmapsets/search")
                .bearer_auth(&token)
                .query(&params)
                .send()
                .await?;

            if !resp.status().is_success() {
                let status = resp.status();
                let text = resp.text().await.unwrap_or_default();
                return Err(anyhow!("API error {}: {}", status, text));
            }
            resp.json().await?
        };

        // Apply local filters (key count, SR, BPM)
        if !filters.key_counts.is_empty()
            || filters.min_sr.is_some()
            || filters.max_sr.is_some()
            || filters.min_bpm.is_some()
            || filters.max_bpm.is_some()
        {
            search_resp.beatmapsets = search_resp
                .beatmapsets
                .into_iter()
                .filter(|bs| {
                    // Key count filter
                    if !filters.key_counts.is_empty() {
                        let keys = bs.key_counts();
                        if !filters.key_counts.iter().any(|k| keys.contains(k)) {
                            return false;
                        }
                    }

                    // Star rating filter
                    if let Some(min) = filters.min_sr {
                        if bs.max_sr() < min {
                            return false;
                        }
                    }
                    if let Some(max) = filters.max_sr {
                        if bs.min_sr() > max {
                            return false;
                        }
                    }

                    // BPM filter
                    if let Some(min) = filters.min_bpm {
                        if bs.bpm.unwrap_or(0.0) < min {
                            return false;
                        }
                    }
                    if let Some(max) = filters.max_bpm {
                        if bs.bpm.unwrap_or(999.0) > max {
                            return false;
                        }
                    }

                    true
                })
                .collect();
        }

        Ok(search_resp)
    }
}
