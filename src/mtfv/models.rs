//! Lenient models of the undocumented mtfv.de JSON (read 2026-11-08).
//! Unknown fields are ignored and most fields default, because the format
//! changes without notice.

use chrono::NaiveDateTime;
use serde::{Deserialize, Deserializer};

/// `spieltag` is a string ("1. Spieltag") in most places and a number in
/// others.
fn string_or_number<'de, D: Deserializer<'de>>(d: D) -> Result<String, D::Error> {
    Ok(match serde_json::Value::deserialize(d)? {
        serde_json::Value::String(s) => s,
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    })
}

#[derive(Debug, Clone, Deserialize)]
pub struct Veranstaltung {
    pub veranstaltung_id: u32,
    #[serde(default)]
    pub bezeichnung: String,
    #[serde(default)]
    pub modus_id: u32,
}

#[derive(Debug, Clone, Default, Deserialize)]
pub struct Modus {
    #[serde(default)]
    pub teamspiel_modus_id: u32,
    #[serde(default)]
    pub bezeichnung: String,
    /// 0: sets, 1: goals count (race).
    #[serde(default)]
    pub punktetyp: u8,
    #[serde(default)]
    pub punkte_sieg_einzel: i32,
    #[serde(default)]
    pub punkte_sieg_doppel: i32,
    /// Segment length in a race, 0 otherwise.
    #[serde(default)]
    pub spielpunkte_bedingung: i32,
    #[serde(default)]
    pub modus: String,
}

/// One entry of `begegnungen` in the season overview.
#[derive(Debug, Clone, Deserialize)]
pub struct BegegnungSummary {
    pub begegnung_id: u32,
    #[serde(default)]
    pub veranstaltung_id: u32,
    #[serde(default)]
    pub heim_team_id: u32,
    #[serde(default)]
    pub gast_team_id: u32,
    #[serde(default)]
    pub heim_name: String,
    #[serde(default)]
    pub gast_name: String,
    #[serde(default)]
    pub zeitpunkt: String,
    #[serde(default, deserialize_with = "string_or_number")]
    pub spieltag: String,
    #[serde(default)]
    pub spielort_name: Option<String>,
    /// Filled in during play, so not a sign that the Begegnung is over.
    #[serde(default)]
    pub heim_punkte: Option<u16>,
    #[serde(default)]
    pub gast_punkte: Option<u16>,
}

impl BegegnungSummary {
    pub fn time(&self) -> Option<NaiveDateTime> {
        parse_zeitpunkt(&self.zeitpunkt)
    }
}

pub fn parse_zeitpunkt(s: &str) -> Option<NaiveDateTime> {
    NaiveDateTime::parse_from_str(s, "%Y-%m-%d %H:%M:%S").ok()
}

#[derive(Debug, Clone, Deserialize)]
pub struct SeasonTabelle {
    #[serde(default)]
    pub modus: Modus,
}

/// One league in the season overview (`?view=sportsmanager&format=json`).
#[derive(Debug, Clone, Deserialize)]
pub struct SeasonLeague {
    pub veranstaltung: Veranstaltung,
    #[serde(default)]
    pub begegnungen: Vec<BegegnungSummary>,
    pub tabelle: SeasonTabelle,
}

#[derive(Debug, Clone, Deserialize)]
pub struct TeamInfo {
    pub team_id: u32,
    #[serde(default)]
    pub teamname: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct Spieler {
    pub spieler_id: u32,
    #[serde(default)]
    pub vorname: String,
    #[serde(default)]
    pub nachname: String,
}

/// One row of `spiele`: a set or a segment.
#[derive(Debug, Clone, Deserialize)]
pub struct Spiel {
    pub teamspiel_id: u64,
    #[serde(default)]
    pub teamspiel_nummer: u32,
    #[serde(default)]
    pub heim_spieler_1_id: Option<u32>,
    #[serde(default)]
    pub heim_spieler_2_id: Option<u32>,
    #[serde(default)]
    pub gast_spieler_1_id: Option<u32>,
    #[serde(default)]
    pub gast_spieler_2_id: Option<u32>,
    #[serde(default)]
    pub teamspiel_heim_punkte: Option<u16>,
    #[serde(default)]
    pub teamspiel_gast_punkte: Option<u16>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BegegnungInfo {
    pub begegnung_id: u32,
    #[serde(default)]
    pub zeitpunkt: String,
    #[serde(default, deserialize_with = "string_or_number")]
    pub spieltag: String,
    #[serde(default)]
    pub heim_punkte: Option<u16>,
    #[serde(default)]
    pub gast_punkte: Option<u16>,
}

/// `data` of `?task=begegnung_spielplan`.
#[derive(Debug, Clone, Deserialize)]
pub struct BegegnungDetail {
    pub veranstaltung: Veranstaltung,
    pub begegnung: BegegnungInfo,
    pub heim_team: TeamInfo,
    pub gast_team: TeamInfo,
    #[serde(default)]
    pub spiele: Vec<Spiel>,
    #[serde(default)]
    pub heim_spieler: Vec<Spieler>,
    #[serde(default)]
    pub gast_spieler: Vec<Spieler>,
    #[serde(default)]
    pub modus: Modus,
}

#[derive(Debug, Clone, Deserialize)]
pub(super) struct DataEnvelope<T> {
    pub data: T,
}
