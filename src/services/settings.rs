use anyhow::{Context, Result};
use directories::ProjectDirs;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

use super::OverlayLibrary;
use crate::competition::{SlotKind, kinds_from_letters};

/// What is played (ADR 0012), as chosen in the main window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompetitionKind {
    Kickertool,
    Mtfv,
    /// "Frei" in the UI.
    Manual,
}

/// What a Frei match is: it decides fields, format and overlay template.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FreiKind {
    /// One player per side, sets entered by hand.
    Singles,
    /// Two players per side, sets entered by hand.
    #[default]
    Doubles,
    /// Two teams, a race to the configured target.
    Race,
}

impl FreiKind {
    /// The overlay selection key of a Frei match of this kind.
    pub fn overlay_key(self) -> &'static str {
        match self {
            FreiKind::Singles => "manual:singles",
            FreiKind::Doubles => "manual:doubles",
            FreiKind::Race => "manual:race",
        }
    }
}

impl CompetitionKind {
    /// The name overlay templates list in `tfc:sources` (ADR 0010).
    pub fn key(self) -> &'static str {
        match self {
            CompetitionKind::Kickertool => "kickertool",
            CompetitionKind::Mtfv => "mtfv",
            CompetitionKind::Manual => "manual",
        }
    }
}

const DEFAULT_PORT: u16 = 8080;
const DEFAULT_REFRESH_INTERVAL: u32 = 30;
const KEYRING_TARGET: &str = "default";
const KEYRING_SERVICE: &str = "TFCStreamManager";
const KEYRING_USERNAME: &str = "api_key";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_refresh_interval")]
    pub refresh_interval: u32,
    /// Before ADR 0010: one overlay file for everything. Read once to move
    /// it into the overlays folder, never written.
    #[serde(default, rename = "overlay_path", skip_serializing)]
    pub legacy_overlay_path: Option<String>,
    /// The overlay folder; `None` is `overlays/` in the config dir.
    #[serde(default)]
    pub overlay_dir: Option<PathBuf>,
    /// Overlay template chosen by hand, by source (ADR 0010).
    #[serde(default)]
    pub overlay_choices: BTreeMap<String, String>,
    #[serde(default)]
    pub competition: Option<CompetitionKind>,
    /// The kind of the last Frei match.
    #[serde(default)]
    pub frei_kind: FreiKind,
    /// The MTFV league last chosen (its id in the current season).
    #[serde(default)]
    pub mtfv_league: Option<u32>,
    /// Races go to this many goals; it does not change during a day.
    #[serde(default = "default_race_target", alias = "mtfv_race_target")]
    pub race_target: u16,
    /// Goals per race segment (MTFV: 6, so 42 has 7 segments).
    #[serde(default = "default_race_segment")]
    pub race_segment: u16,
    /// The game of each segment of a Frei race, `D` or `E`; MTFV races
    /// take theirs from the league.
    #[serde(default = "default_race_games")]
    pub race_games: String,
}

impl Settings {
    /// How many segments a race has: the target in segments, at least one.
    pub fn race_segments(&self) -> usize {
        if self.race_segment == 0 {
            1
        } else {
            usize::from(self.race_target.div_ceil(self.race_segment).max(1))
        }
    }

    /// One game per segment: the chosen ones, cut off or filled up with
    /// doubles when the number of segments differs.
    pub fn race_game_kinds(&self) -> Vec<SlotKind> {
        let mut kinds = kinds_from_letters(&self.race_games);
        kinds.resize(self.race_segments(), SlotKind::Doubles);
        kinds
    }
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

const DEFAULT_RACE_TARGET: u16 = 42;
const DEFAULT_RACE_SEGMENT: u16 = 6;

fn default_race_target() -> u16 {
    DEFAULT_RACE_TARGET
}

/// The MTFV Landesliga: D1 D2 E1 D3 E2 D4 D5.
const DEFAULT_RACE_GAMES: &str = "DDEDEDD";

fn default_race_games() -> String {
    DEFAULT_RACE_GAMES.to_owned()
}

fn default_race_segment() -> u16 {
    DEFAULT_RACE_SEGMENT
}

fn default_refresh_interval() -> u32 {
    DEFAULT_REFRESH_INTERVAL
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            refresh_interval: DEFAULT_REFRESH_INTERVAL,
            legacy_overlay_path: None,
            overlay_dir: None,
            overlay_choices: BTreeMap::new(),
            competition: None,
            frei_kind: FreiKind::default(),
            mtfv_league: None,
            race_target: DEFAULT_RACE_TARGET,
            race_segment: DEFAULT_RACE_SEGMENT,
            race_games: default_race_games(),
        }
    }
}

pub struct SettingsService {
    config_dir: PathBuf,
    settings_path: PathBuf,
    overlays: OverlayLibrary,
}

impl SettingsService {
    fn keyring_entry(&self) -> Result<Entry> {
        Entry::new_with_target(KEYRING_TARGET, KEYRING_SERVICE, KEYRING_USERNAME)
            .context("Failed to create Secret Service entry")
    }

    pub fn new() -> Result<Self> {
        let project_dirs = ProjectDirs::from("com", "benfritsch", "TFCStreamManager")
            .context("Failed to determine config directory")?;

        let config_dir = project_dirs.config_dir().to_path_buf();
        fs::create_dir_all(&config_dir)?;

        let settings_path = config_dir.join("settings.json");

        Ok(Self {
            overlays: OverlayLibrary::new(config_dir.join("overlays")),
            config_dir,
            settings_path,
        })
    }

    pub fn load(&self) -> Settings {
        if self.settings_path.exists() {
            match fs::read_to_string(&self.settings_path) {
                Ok(content) => match serde_json::from_str(&content) {
                    Ok(settings) => return settings,
                    Err(e) => {
                        tracing::warn!("Failed to parse settings: {}", e);
                    }
                },
                Err(e) => {
                    tracing::warn!("Failed to read settings file: {}", e);
                }
            }
        }
        Settings::default()
    }

    pub fn save(&self, settings: &Settings) -> Result<()> {
        let content = serde_json::to_string_pretty(settings)?;
        fs::write(&self.settings_path, content)?;
        Ok(())
    }

    /// Load API key from system keyring
    pub fn load_api_key(&self) -> Option<String> {
        if let Ok(entry) = self.keyring_entry()
            && let Ok(password) = entry.get_password()
        {
            return Some(password);
        }

        None
    }

    /// Save API key to system keyring
    pub fn save_api_key(&self, api_key: &str) -> Result<()> {
        if api_key.is_empty() {
            return self.delete_api_key();
        }

        let entry = self.keyring_entry()?;
        entry
            .set_password(api_key)
            .map_err(|e| anyhow::anyhow!("Failed to save API key to keyring: {}", e))
    }

    pub fn delete_api_key(&self) -> Result<()> {
        if let Ok(entry) = self.keyring_entry() {
            let _ = entry.delete_credential();
        }

        Ok(())
    }

    /// Optional overrides for the bundled overlay names.
    pub fn translations_path(&self) -> PathBuf {
        self.config_dir.join("translations.toml")
    }

    /// Where the match on the table and its event log are persisted.
    pub fn table_match_path(&self) -> PathBuf {
        self.config_dir.join("table_match.json")
    }

    /// The overlay templates (ADR 0010). One library, shared by every
    /// caller, so [`Self::use_overlay_dir`] applies everywhere at once.
    pub fn overlays(&self) -> OverlayLibrary {
        self.overlays.clone()
    }

    /// The overlay folder the settings name, or the default one.
    pub fn overlay_dir(&self, settings: &Settings) -> PathBuf {
        settings
            .overlay_dir
            .clone()
            .unwrap_or_else(|| self.config_dir.join("overlays"))
    }

    /// Switches to the settings' overlay folder.
    pub fn use_overlay_dir(&self, settings: &Settings) {
        self.overlays.set_dir(self.overlay_dir(settings));
    }

    /// Moves the overlay configured before ADR 0010 into the overlays folder:
    /// the `overlay_path` setting, or the `player_overlay.html` that used to
    /// override the bundled one in the config dir. Returns what to log.
    pub fn migrate_overlay_path(&self, settings: &mut Settings) -> Option<String> {
        let legacy_default = self.config_dir.join("player_overlay.html");
        let (file, move_file) = match settings.legacy_overlay_path.take() {
            Some(path) => (PathBuf::from(path), false),
            None if legacy_default.exists() => (legacy_default, true),
            None => return None,
        };
        let message = match self.overlays().adopt(&file, move_file) {
            Ok(Some(target)) => format!("Overlay {} moved to {}", file.display(), target.display()),
            Ok(None) => format!("Overlay {} is already available", file.display()),
            Err(e) => format!("Overlay {} not migrated: {e}", file.display()),
        };
        if let Err(e) = self.save(settings) {
            return Some(format!("{message}; settings not saved: {e}"));
        }
        Some(message)
    }
}

impl Default for SettingsService {
    fn default() -> Self {
        Self::new().expect("Failed to create settings service")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_settings() {
        let s = Settings::default();
        assert_eq!(s.port, 8080);
        assert_eq!(s.refresh_interval, 30);
        assert!(s.legacy_overlay_path.is_none());
        assert!(s.overlay_choices.is_empty());
    }

    #[test]
    fn deserialize_full() {
        let json = r#"{
            "port": 9090,
            "refresh_interval": 15,
            "overlay_path": "/tmp/overlay.html",
            "show_sets": false
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.port, 9090);
        assert_eq!(s.refresh_interval, 15);
        assert_eq!(s.legacy_overlay_path, Some("/tmp/overlay.html".into()));
    }

    #[test]
    fn race_games_follow_the_segments() {
        let mut s = Settings::default();
        assert_eq!(s.race_segments(), 7);
        assert_eq!(s.race_game_kinds().len(), 7);
        s.race_segment = 5;
        let kinds = s.race_game_kinds();
        assert_eq!(kinds.len(), 9, "42 in fives");
        assert_eq!(kinds[7..], [SlotKind::Doubles, SlotKind::Doubles]);
        s.race_segment = 0;
        assert_eq!(
            s.race_game_kinds(),
            [SlotKind::Doubles],
            "no segments: one game"
        );
    }

    #[test]
    fn deserialize_empty_uses_defaults() {
        let json = r#"{}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.port, 8080);
        assert_eq!(s.refresh_interval, 30);
    }

    #[test]
    fn serialize_roundtrip() {
        let s = Settings {
            port: 3000,
            refresh_interval: 5,
            legacy_overlay_path: Some("/custom.html".into()),
            overlay_dir: Some("/srv/overlays".into()),
            overlay_choices: BTreeMap::from([("kickertool".into(), "player_overlay".into())]),
            competition: Some(CompetitionKind::Mtfv),
            frei_kind: FreiKind::Race,
            mtfv_league: Some(72),
            race_target: 30,
            race_segment: 5,
            race_games: "DE".into(),
        };
        let json = serde_json::to_string(&s).unwrap();
        assert!(!json.contains("overlay_path"));
        let s2: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s2.overlay_choices, s.overlay_choices);
        assert_eq!(s2.overlay_dir, s.overlay_dir);
        assert_eq!(s2.competition, s.competition);
        assert_eq!((s2.race_target, s2.race_segment), (30, 5));
        let old: Settings = serde_json::from_str(r#"{"mtfv_race_target": 36}"#).unwrap();
        assert_eq!((old.race_target, old.race_segment), (36, 6));
        assert_eq!(s2.port, 3000);
        assert_eq!(s2.refresh_interval, 5);
    }
}
