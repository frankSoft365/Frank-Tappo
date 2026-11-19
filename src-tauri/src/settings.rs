use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const SCALE_MIN: f64 = 0.5;
pub const SCALE_MAX: f64 = 1.5;
pub const SCALE_DEFAULT: f64 = 0.75;

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_scale")]
    pub scale: f64,
}

fn default_language() -> String {
    "system".into()
}

fn default_scale() -> f64 {
    SCALE_DEFAULT
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: default_language(),
            scale: SCALE_DEFAULT,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct SettingsView {
    pub language: String,
    pub resolved: String,
    pub scale: f64,
    pub scale_min: f64,
    pub scale_max: f64,
}

pub fn system_language() -> &'static str {
    match sys_locale::get_locale() {
        Some(locale) if locale.to_ascii_lowercase().starts_with("zh") => "zh",
        _ => "en",
    }
}

pub fn resolve(language: &str) -> &'static str {
    match language {
        "zh" => "zh",
        "en" => "en",
        _ => system_language(),
    }
}

pub fn view(settings: &Settings) -> SettingsView {
    SettingsView {
        language: settings.language.clone(),
        resolved: resolve(&settings.language).into(),
        scale: settings.scale.clamp(SCALE_MIN, SCALE_MAX),
        scale_min: SCALE_MIN,
        scale_max: SCALE_MAX,
    }
}

pub fn load(app: &AppHandle) -> Settings {
    let mut settings = fs::read_to_string(path(app))
        .ok()
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .unwrap_or_default();
    settings.scale = settings.scale.clamp(SCALE_MIN, SCALE_MAX);
    settings
}

pub fn save(app: &AppHandle, settings: &Settings) -> Result<(), String> {
    let path = path(app);
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let text = serde_json::to_string_pretty(settings).map_err(|error| error.to_string())?;
    fs::write(path, text).map_err(|error| error.to_string())
}

fn path(app: &AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("settings.json")
}
