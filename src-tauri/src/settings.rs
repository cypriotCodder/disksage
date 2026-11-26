use std::path::PathBuf;

use serde::{Deserialize, Serialize};

pub const DEFAULT_MODEL: &str = "claude-haiku-5-5";

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Anthropic API key. Stored in %APPDATA%\com.nedim.disksage\settings.json.
    pub api_key: String,
    pub model: String,
    /// Replace C:\Users\<you> with C:\Users\%USER% before anything is sent to the AI.
    pub redact_username: bool,
    /// Items per AI request.
    pub batch_size: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            api_key: String::new(),
            model: DEFAULT_MODEL.into(),
            redact_username: true,
            batch_size: 40,
        }
    }
}

/// What the UI is allowed to see: never the key itself.
#[derive(Debug, Clone, Serialize)]
pub struct SettingsView {
    pub has_key: bool,
    pub key_hint: String,
    pub model: String,
    pub redact_username: bool,
    pub settings_path: String,
}

impl Settings {
    pub fn view(&self, path: &PathBuf) -> SettingsView {
        let k = self.api_key.trim();
        let chars: Vec<char> = k.chars().collect();
        let key_hint = if chars.len() > 12 {
            let head: String = chars[..7].iter().collect();
            let tail: String = chars[chars.len() - 4..].iter().collect();
            format!("{head}…{tail}")
        } else {
            String::new()
        };
        SettingsView {
            has_key: !k.is_empty(),
            key_hint,
            model: self.model.clone(),
            redact_username: self.redact_username,
            settings_path: path.to_string_lossy().into_owned(),
        }
    }

    pub fn load(path: &PathBuf) -> Settings {
        let mut s: Settings = std::fs::read_to_string(path)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        // An environment variable wins, handy during development.
        if let Ok(k) = std::env::var("ANTHROPIC_API_KEY") {
            if !k.trim().is_empty() {
                s.api_key = k;
            }
        }
        if s.model.trim().is_empty() {
            s.model = DEFAULT_MODEL.into();
        }
        s.batch_size = s.batch_size.clamp(5, 80);
        s
    }

    pub fn save(&self, path: &PathBuf) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
    }
}
