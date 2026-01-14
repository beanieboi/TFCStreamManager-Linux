use anyhow::{Context, Result};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64};
use directories::ProjectDirs;
use keyring::Entry;
use serde::{Deserialize, Serialize};
use std::fs;
use std::os::unix::fs::PermissionsExt;
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
    secrets_path: PathBuf,
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
        let secrets_path = config_dir.join(".secrets");

        Ok(Self {
            config_dir,
            settings_path,
            secrets_path,
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

    /// Load API key - tries system keyring first, then falls back to encrypted file
    pub fn load_api_key(&self) -> Option<String> {
        // Try system keyring first
        if let Ok(entry) = self.keyring_entry()
            && let Ok(password) = entry.get_password()
        {
            return Some(password);
        }

        // Fall back to encrypted file
        if let Some(key) = self.load_api_key_from_file() {
            return Some(key);
        }

        None
    }

    /// Save API key - tries system keyring first, then falls back to encrypted file
    pub fn save_api_key(&self, api_key: &str) -> Result<()> {
        if api_key.is_empty() {
            return self.delete_api_key();
        }

        // Try system keyring first
        if let Ok(entry) = self.keyring_entry()
            && entry.set_password(api_key).is_ok()
        {
            // Also remove any file-based key if it exists
            let _ = fs::remove_file(&self.secrets_path);
            return Ok(());
        }

        // Fall back to encrypted file
        self.save_api_key_to_file(api_key)
    }

    pub fn delete_api_key(&self) -> Result<()> {
        // Try to delete from keyring
        if let Ok(entry) = self.keyring_entry() {
            let _ = entry.delete_credential();
        }

        // Also delete file-based key if it exists
        if self.secrets_path.exists() {
            fs::remove_file(&self.secrets_path)?;
        }

        Ok(())
    }

    /// Get a machine-specific key for encryption
    /// Uses /etc/machine-id on Linux, falls back to hostname + username
    fn get_encryption_key(&self) -> Vec<u8> {
        // Try to read machine-id (available on most Linux systems)
        if let Ok(machine_id) = fs::read_to_string("/etc/machine-id") {
            return machine_id.trim().as_bytes().to_vec();
        }

        // Fallback: use hostname + username + app name
        let hostname = hostname::get()
            .map(|h| h.to_string_lossy().into_owned())
            .unwrap_or_else(|_| "localhost".to_owned());

        let username = std::env::var("USER")
            .or_else(|_| std::env::var("USERNAME"))
            .unwrap_or_else(|_| "user".to_string());

        format!("TFCStreamManager-{}-{}", hostname, username)
            .as_bytes()
            .to_vec()
    }

    /// Simple XOR encryption with the machine key
    fn encrypt(&self, data: &[u8]) -> Vec<u8> {
        let key = self.get_encryption_key();
        data.iter()
            .enumerate()
            .map(|(i, &b)| b ^ key[i % key.len()])
            .collect()
    }

    /// Simple XOR decryption (same as encryption for XOR)
    fn decrypt(&self, data: &[u8]) -> Vec<u8> {
        self.encrypt(data) // XOR is symmetric
    }

    fn save_api_key_to_file(&self, api_key: &str) -> Result<()> {
        let encrypted = self.encrypt(api_key.as_bytes());
        let encoded = BASE64.encode(&encrypted);

        fs::write(&self.secrets_path, &encoded)?;

        // Set restrictive permissions (owner read/write only)
        let metadata = fs::metadata(&self.secrets_path)?;
        let mut permissions = metadata.permissions();
        permissions.set_mode(0o600);
        fs::set_permissions(&self.secrets_path, permissions)?;

        Ok(())
    }

    fn load_api_key_from_file(&self) -> Option<String> {
        if !self.secrets_path.exists() {
            return None;
        }

        let encoded = fs::read_to_string(&self.secrets_path).ok()?;
        let encrypted = BASE64.decode(encoded.trim()).ok()?;
        let decrypted = self.decrypt(&encrypted);
        String::from_utf8(decrypted).ok()
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
