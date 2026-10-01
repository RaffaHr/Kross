// Preferences, stored as plain JSON in %APPDATA%\Coucou\settings.json.
// No secret ever lands here — API keys live in the Windows Credential Manager.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub sound_enabled: bool,
    pub sound_volume: f64,
    pub auto_close_interval: f64,
    pub absence_interval: f64,
    pub active_integrations: Vec<String>,
    /// "primary" = the main display, "cursor" = whichever display the mouse is on.
    pub screen: String,
    pub autostart: bool,
    pub hooks_installed: bool,
    /// Claude model used by the chat — legacy field; the multi-provider
    /// equivalent is `provider_models["claude"]`, which wins when set.
    /// Defaulted explicitly so a settings.json written by an older build still loads.
    #[serde(default = "default_model")]
    pub model: String,
    /// Which provider drives `chat_send` — one of providers::PROVIDERS ids.
    #[serde(default = "default_active_provider")]
    pub active_provider: String,
    /// Per-provider model overrides; absent entries use the provider default.
    #[serde(default)]
    pub provider_models: std::collections::HashMap<String, String>,
    /// Base URL for the `custom` OpenAI-compatible provider — not a secret.
    #[serde(default)]
    pub custom_base_url: String,
    /// Google's OAuth client registration, pasted by the user in Settings →
    /// Providers. These are *public installed-app* values (the same pair every
    /// Gemini-CLI install ships) kept out of the binary because secret
    /// scanners flag the literals — they are not user credentials, so plain
    /// settings storage is fine. Env vars still override (see the spec).
    #[serde(default)]
    pub google_client_id: String,
    #[serde(default)]
    pub google_client_secret: String,
}

impl Settings {
    /// User-supplied OAuth client registration for providers whose public
    /// client can't ship in the binary. Returns the (id, secret) pair only
    /// when both are set — a lone id would die at the token endpoint anyway.
    pub fn oauth_client(&self, provider: &str) -> Option<(String, String)> {
        let (id, secret) = match provider {
            "google" => (self.google_client_id.trim(), self.google_client_secret.trim()),
            _ => return None,
        };
        (!id.is_empty() && !secret.is_empty()).then(|| (id.to_string(), secret.to_string()))
    }
}

fn default_model() -> String {
    "claude-opus-5".to_string()
}

fn default_active_provider() -> String {
    "claude".to_string()
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            sound_enabled: true,
            sound_volume: 0.12,
            auto_close_interval: 15.0,
            absence_interval: 180.0,
            active_integrations: vec![
                "integration_resend".into(),
                "integration_n8n".into(),
                "integration_vercel".into(),
                "integration_github".into(),
            ],
            screen: "primary".into(),
            autostart: false,
            hooks_installed: false,
            model: default_model(),
            active_provider: default_active_provider(),
            provider_models: std::collections::HashMap::new(),
            custom_base_url: String::new(),
            google_client_id: String::new(),
            google_client_secret: String::new(),
        }
    }
}

/// %APPDATA%\Coucou
pub fn config_dir() -> PathBuf {
    let base = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Coucou")
}

/// %LOCALAPPDATA%\Coucou — where coucou-hook.exe and the log live.
pub fn local_dir() -> PathBuf {
    let base = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join("Coucou")
}

pub fn hook_exe_path() -> PathBuf {
    local_dir().join("bin").join("coucou-hook.exe")
}

fn settings_path() -> PathBuf {
    config_dir().join("settings.json")
}

pub fn load() -> Settings {
    match std::fs::read(settings_path()) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => Settings::default(),
    }
}

pub fn save(settings: &Settings) -> std::io::Result<()> {
    let dir = config_dir();
    std::fs::create_dir_all(&dir)?;
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    std::fs::write(settings_path(), json)
}
