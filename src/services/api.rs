use anyhow::{Context, Result};
use reqwest::{Client, header};
use serde::de::DeserializeOwned;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::SettingsService;
use crate::models::{PaginatedResponse, Table, Tournament};

const BASE_URL: &str = "https://api.tournament.io/v1/public/";
const VALID_TOURNAMENT_STATES: [&str; 5] = [
    "planned",
    "pre-registration",
    "check-in",
    "ready",
    "running",
];

pub struct KickertoolApiService {
    client: Client,
    api_key: Arc<RwLock<Option<String>>>,
}

impl KickertoolApiService {
    pub fn new(settings_service: Arc<SettingsService>) -> Result<Self> {
        let api_key = settings_service.load_api_key();

        let mut headers = header::HeaderMap::new();
        headers.insert(
            header::ACCEPT,
            header::HeaderValue::from_static("application/json"),
        );

        let client = Client::builder().default_headers(headers).build()?;

        Ok(Self {
            client,
            api_key: Arc::new(RwLock::new(api_key)),
        })
    }

    pub async fn update_api_key(&self, api_key: String) {
        let mut key = self.api_key.write().await;
        if api_key.is_empty() {
            *key = None;
        } else {
            *key = Some(api_key);
        }
    }

    async fn get_auth_header(&self) -> Option<String> {
        self.api_key.read().await.clone()
    }

    pub async fn get<T: DeserializeOwned>(&self, endpoint: &str) -> Result<T> {
        let url = format!("{}{}", BASE_URL, endpoint);

        let mut request = self.client.get(&url);

        if let Some(api_key) = self.get_auth_header().await {
            request = request.bearer_auth(api_key);
        }

        let response = request.send().await.context("Failed to send request")?;

        let status = response.status();
        if !status.is_success() {
            let error_text = response.text().await.unwrap_or_default();
            anyhow::bail!("API request failed with status {}: {}", status, error_text);
        }

        let text = response.text().await?;
        serde_json::from_str(&text).context("Failed to parse response")
    }

    pub async fn get_all_paginated<T: DeserializeOwned + Clone>(
        &self,
        endpoint: &str,
        page_size: i32,
    ) -> Result<Vec<T>> {
        let mut all_items = Vec::new();
        let mut offset = 0;
        let max_empty_pages = 2;
        let mut empty_page_count = 0;

        loop {
            let separator = if endpoint.contains('?') { "&" } else { "?" };
            let paginated_endpoint =
                format!("{}{separator}limit={page_size}&offset={offset}", endpoint);

            let url = format!("{}{}", BASE_URL, paginated_endpoint);

            let mut request = self.client.get(&url);
            if let Some(api_key) = self.get_auth_header().await {
                request = request.bearer_auth(api_key);
            }

            let response = request.send().await?;

            if !response.status().is_success() {
                break;
            }

            let text = response.text().await?;
            let trimmed = text.trim();

            if trimmed.starts_with('[') {
                // Direct array response
                let items: Vec<T> = serde_json::from_str(&text)?;
                if items.is_empty() {
                    empty_page_count += 1;
                    if empty_page_count >= max_empty_pages {
                        break;
                    }
                } else {
                    let count = items.len();
                    all_items.extend(items);
                    offset += page_size;
                    empty_page_count = 0;
                    if count < page_size as usize {
                        break;
                    }
                }
            } else {
                // Try parsing as PaginatedResponse
                match serde_json::from_str::<PaginatedResponse<Vec<T>>>(&text) {
                    Ok(paginated) => {
                        let has_more = paginated.has_more();
                        all_items.extend(paginated.data);
                        if !has_more {
                            break;
                        }
                        offset += page_size;
                    }
                    Err(_) => break,
                }
            }
        }

        Ok(all_items)
    }

    pub async fn load_tournaments_with_tables(
        &self,
    ) -> Result<(
        Vec<Tournament>,
        std::collections::HashMap<String, Vec<Table>>,
    )> {
        let mut tournaments = self
            .get_all_paginated::<Tournament>("tournaments", 50)
            .await?;
        tournaments.retain(|t| VALID_TOURNAMENT_STATES.contains(&t.state.as_str()));

        let mut all_tables = std::collections::HashMap::new();
        for tournament in &tournaments {
            if let Ok(tournament_tables) = self
                .get::<Vec<Table>>(&format!("tournaments/{}/courts", tournament.id))
                .await
            {
                let mut sorted = tournament_tables;
                sorted.sort_by_key(|t| t.number);
                all_tables.insert(tournament.id.clone(), sorted);
            }
        }

        Ok((tournaments, all_tables))
    }
}
