//! The player's choices kept between runs: voice and blend, speed, volume,
//! the language to translate into, how to save, and the speech API, in
//! `vinospeak/gui.json` under the config folder.

use std::path::{Path, PathBuf};

use serde_json::json;

#[derive(Debug, Clone, PartialEq)]
pub struct Settings {
    pub voice: Option<String>,
    pub speed: Option<f32>,
    pub volume: Option<f32>,
    /// vinospeak's name for the language, as `--translate` takes it.
    pub translate: Option<String>,
    /// The voice mixed in, and how many tenths of it.
    pub blend: Option<String>,
    pub mix: Option<u8>,
    /// The suffix speech is saved with, and whether subtitles go beside it.
    pub save_format: Option<String>,
    pub subtitles: Option<bool>,
    /// Whether `vinospeak --http` runs while the window is open.
    pub api: Option<bool>,
}

/// `$XDG_CONFIG_HOME`, else `~/.config`, then `vinospeak`: where vinospeak
/// looks for lexicon.toml too.
pub fn config_dir() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))?;
    Some(config.join("vinospeak"))
}

pub fn path() -> Option<PathBuf> {
    Some(config_dir()?.join("gui.json"))
}

/// What was saved; a field missing, of the wrong type or out of range is None,
/// and so is all of it when the file is missing or isn't JSON.
pub fn load(path: &Path) -> Settings {
    let saved: serde_json::Value = std::fs::read_to_string(path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let number = |key: &str| {
        saved
            .get(key)
            .and_then(serde_json::Value::as_f64)
            .map(|n| n as f32)
    };
    let text = |key: &str| {
        saved
            .get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let flag = |key: &str| saved.get(key).and_then(serde_json::Value::as_bool);
    Settings {
        voice: text("voice"),
        speed: number("speed"),
        volume: number("volume").filter(|v| (0.0..=1.0).contains(v)),
        translate: text("translate"),
        blend: text("blend"),
        mix: saved
            .get("mix")
            .and_then(serde_json::Value::as_u64)
            .and_then(|m| u8::try_from(m).ok()),
        save_format: text("save_format"),
        subtitles: flag("subtitles"),
        api: flag("api"),
    }
}

/// Written whole beside the old file then renamed over it, so a crash
/// mid-write leaves the old choices.
pub fn save(path: &Path, settings: &Settings) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let text = json!({
        "voice": settings.voice,
        "speed": settings.speed,
        "volume": settings.volume,
        "translate": settings.translate,
        "blend": settings.blend,
        "mix": settings.mix,
        "save_format": settings.save_format,
        "subtitles": settings.subtitles,
        "api": settings.api,
    });
    let partial = path.with_extension("json.partial");
    std::fs::write(&partial, format!("{text:#}\n"))?;
    std::fs::rename(&partial, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(name: &str) -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("vinospeak-settings-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        dir.join("vinospeak/gui.json")
    }

    #[test]
    fn what_is_saved_is_loaded_back() {
        let path = scratch("round-trip");
        let settings = Settings {
            voice: Some("bm_george".to_owned()),
            speed: Some(1.25),
            volume: Some(0.4),
            translate: Some("es".to_owned()),
            blend: Some("af_heart".to_owned()),
            mix: Some(3),
            save_format: Some("opus".to_owned()),
            subtitles: Some(true),
            api: Some(false),
        };
        save(&path, &settings).unwrap();
        assert_eq!(load(&path), settings);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_missing_or_broken_file_loads_as_nothing_chosen() {
        let path = scratch("broken");
        let nothing = Settings {
            voice: None,
            speed: None,
            volume: None,
            translate: None,
            blend: None,
            mix: None,
            save_format: None,
            subtitles: None,
            api: None,
        };
        assert_eq!(load(&path), nothing);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "not json").unwrap();
        assert_eq!(load(&path), nothing);
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }

    #[test]
    fn a_volume_from_0_to_1_is_kept_and_one_outside_is_dropped() {
        let path = scratch("volume");
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        for (volume, kept) in [
            ("0", Some(0.0)),
            ("1", Some(1.0)),
            ("1.5", None),
            ("-0.1", None),
        ] {
            std::fs::write(&path, format!(r#"{{"volume": {volume}, "speed": "fast"}}"#)).unwrap();
            let loaded = load(&path);
            assert_eq!(loaded.volume, kept, "{volume}");
            assert_eq!(loaded.speed, None);
        }
        let _ = std::fs::remove_dir_all(path.parent().unwrap().parent().unwrap());
    }
}
