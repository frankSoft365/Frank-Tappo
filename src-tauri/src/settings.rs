use std::fs;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Manager};

pub const SCALE_MIN: f64 = 0.24;
pub const SCALE_MAX: f64 = 0.6;
pub const SCALE_DEFAULT: f64 = 0.4;
const SCALE_STEP: f64 = SCALE_DEFAULT / 100.0;
pub const OPACITY_DEFAULT: f64 = 1.0;

pub fn normalize_scale(scale: f64) -> f64 {
    (scale.clamp(SCALE_MIN, SCALE_MAX) / SCALE_STEP).round() * SCALE_STEP
}

pub fn normalize_opacity(opacity: f64) -> f64 {
    if !opacity.is_finite() {
        return OPACITY_DEFAULT;
    }
    (opacity.clamp(0.0, 1.0) * 100.0).round() / 100.0
}

#[derive(Clone, Serialize, Deserialize)]
pub struct Settings {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default = "default_skin")]
    pub skin: String,
    #[serde(default)]
    pub accessory: String,
    #[serde(default = "default_scale")]
    pub scale: f64,
    #[serde(default)]
    pub drag_disabled: bool,
    #[serde(default)]
    pub hidden: bool,
    #[serde(default = "default_opacity")]
    pub opacity: f64,
}

fn default_language() -> String {
    "system".into()
}

fn default_skin() -> String {
    "xigerika".into()
}

fn default_scale() -> f64 {
    SCALE_DEFAULT
}

fn default_opacity() -> f64 {
    OPACITY_DEFAULT
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: default_language(),
            skin: default_skin(),
            accessory: String::new(),
            scale: SCALE_DEFAULT,
            drag_disabled: false,
            hidden: false,
            opacity: OPACITY_DEFAULT,
        }
    }
}

#[derive(Clone, Serialize)]
pub struct SettingsView {
    pub language: String,
    pub resolved: String,
    pub skin: String,
    pub accessory: String,
    pub scale: f64,
    pub scale_min: f64,
    pub scale_max: f64,
    pub drag_disabled: bool,
    pub hidden: bool,
    pub opacity: f64,
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
        skin: settings.skin.clone(),
        accessory: settings.accessory.clone(),
        scale: normalize_scale(settings.scale),
        scale_min: SCALE_MIN,
        scale_max: SCALE_MAX,
        drag_disabled: settings.drag_disabled,
        hidden: settings.hidden,
        opacity: normalize_opacity(settings.opacity),
    }
}

pub fn load(app: &AppHandle) -> Settings {
    let mut settings = fs::read_to_string(path(app))
        .ok()
        .and_then(|text| serde_json::from_str::<Settings>(&text).ok())
        .unwrap_or_default();
    settings.scale = normalize_scale(settings.scale);
    settings.opacity = normalize_opacity(settings.opacity);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_scale_and_range() {
        let settings = Settings::default();
        let view = view(&settings);
        assert_eq!(settings.scale, 0.4);
        assert_eq!(view.scale, 0.4);
        assert_eq!(view.scale_min, 0.24);
        assert_eq!(view.scale_max, 0.6);
        assert_eq!(view.accessory, "");
        assert!(!view.drag_disabled);
        assert!(!view.hidden);
        assert_eq!(view.opacity, 1.0);
    }

    #[test]
    fn pet_visibility_flags_and_opacity_survive_round_trip() {
        let old: Settings =
            serde_json::from_str(r#"{"language":"system","skin":"xigerika","scale":0.4}"#)
                .unwrap();
        assert!(!old.drag_disabled);
        assert!(!old.hidden);
        assert_eq!(old.opacity, 1.0);

        let settings = Settings {
            drag_disabled: true,
            hidden: true,
            opacity: 0.35,
            ..Settings::default()
        };
        let saved = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&saved).unwrap();
        assert!(loaded.drag_disabled);
        assert!(loaded.hidden);
        assert_eq!(view(&loaded).opacity, 0.35);
    }

    #[test]
    fn opacity_is_clamped_to_percent_steps() {
        assert_eq!(normalize_opacity(-0.5), 0.0);
        assert_eq!(normalize_opacity(1.5), 1.0);
        assert_eq!(normalize_opacity(0.734), 0.73);
        assert_eq!(normalize_opacity(f64::NAN), 1.0);
    }

    #[test]
    fn accessory_defaults_to_none_and_survives_round_trip() {
        let old: Settings = serde_json::from_str(
            r#"{"language":"system","skin":"xigerika","scale":0.4}"#,
        )
        .unwrap();
        assert_eq!(view(&old).accessory, "");

        let settings = Settings {
            accessory: "dragon".into(),
            ..Settings::default()
        };
        let saved = serde_json::to_string(&settings).unwrap();
        let loaded: Settings = serde_json::from_str(&saved).unwrap();
        assert_eq!(view(&loaded).accessory, "dragon");
    }

    #[test]
    fn scale_uses_integer_percentages_of_the_default_size() {
        assert_eq!(normalize_scale(0.2), 0.24);
        assert_eq!(normalize_scale(0.53), 0.532);
        assert_eq!(normalize_scale(0.529), 0.528);
        assert_eq!(normalize_scale(0.601), 0.6);
        for percent in 60..=150 {
            let scale = normalize_scale(percent as f64 * SCALE_DEFAULT / 100.0);
            assert!((scale / SCALE_DEFAULT * 100.0 - percent as f64).abs() < 1e-9);
            assert_eq!(normalize_scale(scale), scale);
            let settings = Settings {
                language: "system".into(),
                skin: default_skin(),
                accessory: String::new(),
                scale,
                ..Settings::default()
            };
            let saved = serde_json::to_string(&settings).unwrap();
            let loaded: Settings = serde_json::from_str(&saved).unwrap();
            assert_eq!(view(&loaded).scale, scale);
        }
    }

    #[test]
    fn scale_is_clamped_to_range() {
        let mut settings = Settings::default();
        settings.scale = 0.1;
        assert_eq!(view(&settings).scale, 0.24);
        settings.scale = 0.8;
        assert_eq!(view(&settings).scale, 0.6);
    }
}
