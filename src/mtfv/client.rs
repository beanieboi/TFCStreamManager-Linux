use anyhow::{Context, Result};
use reqwest::Client;

use super::models::{BegegnungDetail, DataEnvelope, SeasonLeague};

const BASE_URL: &str = "https://mtfv.de/ligabetrieb/aktuelle-saison";
const USER_AGENT: &str = concat!("TFCStreamManager/", env!("CARGO_PKG_VERSION"));

pub struct MtfvClient {
    client: Client,
    base_url: String,
}

impl MtfvClient {
    pub fn new() -> Result<Self> {
        Self::with_base_url(BASE_URL)
    }

    pub fn with_base_url(base_url: &str) -> Result<Self> {
        let client = Client::builder()
            .user_agent(USER_AGENT)
            .timeout(std::time::Duration::from_secs(15))
            .build()?;
        Ok(Self {
            client,
            base_url: base_url.trim_end_matches('/').to_owned(),
        })
    }

    async fn fetch(&self, query: &[(&str, String)]) -> Result<String> {
        let response = self
            .client
            .get(&self.base_url)
            .query(query)
            .query(&[("format", "json")])
            .send()
            .await
            .context("mtfv.de request failed")?
            .error_for_status()
            .context("mtfv.de returned an error")?;
        response.text().await.context("reading mtfv.de response")
    }

    /// All leagues of the current season with their Begegnungen.
    pub async fn season(&self) -> Result<Vec<SeasonLeague>> {
        let body = self.fetch(&[("view", "sportsmanager".into())]).await?;
        parse_season(&body)
    }

    /// One Begegnung with rosters, `spiele` and `modus`.
    pub async fn begegnung(
        &self,
        veranstaltung_id: u32,
        begegnung_id: u32,
    ) -> Result<BegegnungDetail> {
        let body = self
            .fetch(&[
                ("task", "begegnung_spielplan".into()),
                ("veranstaltungid", veranstaltung_id.to_string()),
                ("id", begegnung_id.to_string()),
            ])
            .await?;
        parse_begegnung(&body)
    }
}

pub(super) fn parse_season(body: &str) -> Result<Vec<SeasonLeague>> {
    serde_json::from_str(body).context("parsing mtfv.de season overview")
}

pub(super) fn parse_begegnung(body: &str) -> Result<BegegnungDetail> {
    let envelope: DataEnvelope<BegegnungDetail> =
        serde_json::from_str(body).context("parsing mtfv.de Begegnung")?;
    Ok(envelope.data)
}
