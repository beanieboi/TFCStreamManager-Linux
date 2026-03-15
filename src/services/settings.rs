use anyhow::{Context, Result};
use directories::ProjectDirs;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

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
    #[serde(default)]
    pub overlay_path: Option<String>,
    #[serde(default = "default_true")]
    pub show_sets: bool,
    #[serde(default = "default_true")]
    pub show_score: bool,
}

fn default_port() -> u16 {
    DEFAULT_PORT
}

fn default_refresh_interval() -> u32 {
    DEFAULT_REFRESH_INTERVAL
}

fn default_true() -> bool {
    true
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            port: DEFAULT_PORT,
            refresh_interval: DEFAULT_REFRESH_INTERVAL,
            overlay_path: None,
            show_sets: true,
            show_score: true,
        }
    }
}

pub struct SettingsService {
    config_dir: PathBuf,
    settings_path: PathBuf,
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

    pub fn get_default_overlay_path(&self) -> PathBuf {
        // Look for overlay in config dir first, then in executable directory
        let config_overlay = self.config_dir.join("player_overlay.html");
        if config_overlay.exists() {
            return config_overlay;
        }

        // Try executable directory
        if let Ok(exe_path) = std::env::current_exe()
            && let Some(exe_dir) = exe_path.parent()
        {
            let exe_overlay = exe_dir.join("player_overlay.html");
            if exe_overlay.exists() {
                return exe_overlay;
            }
        }

        // Try current directory
        let cwd_overlay = PathBuf::from("player_overlay.html");
        if cwd_overlay.exists() {
            return cwd_overlay;
        }

        // Default to config dir path even if it doesn't exist
        config_overlay
    }

    pub fn get_overlay_path(&self, settings: &Settings) -> PathBuf {
        settings
            .overlay_path
            .as_ref()
            .map(PathBuf::from)
            .unwrap_or_else(|| self.get_default_overlay_path())
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
        assert!(s.overlay_path.is_none());
        assert!(s.show_sets);
        assert!(s.show_score);
    }

    #[test]
    fn deserialize_full() {
        let json = r#"{
            "port": 9090,
            "refresh_interval": 15,
            "overlay_path": "/tmp/overlay.html",
            "show_sets": false,
            "show_score": true
        }"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.port, 9090);
        assert_eq!(s.refresh_interval, 15);
        assert_eq!(s.overlay_path, Some("/tmp/overlay.html".into()));
        assert!(!s.show_sets);
        assert!(s.show_score);
    }

    #[test]
    fn deserialize_empty_uses_defaults() {
        let json = r#"{}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.port, 8080);
        assert_eq!(s.refresh_interval, 30);
        assert!(s.show_sets);
        assert!(s.show_score);
    }

    #[test]
    fn serialize_roundtrip() {
        let s = Settings {
            port: 3000,
            refresh_interval: 5,
            overlay_path: Some("/custom.html".into()),
            show_sets: false,
            show_score: false,
        };
        let json = serde_json::to_string(&s).unwrap();
        let s2: Settings = serde_json::from_str(&json).unwrap();
        assert_eq!(s2.port, 3000);
        assert_eq!(s2.refresh_interval, 5);
        assert!(!s2.show_sets);
        assert!(!s2.show_score);
    }
}
