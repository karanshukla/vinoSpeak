//! The player's choices kept between runs: voice, speed, volume and the
//! language to translate into, in
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
}

/// `$XDG_CONFIG_HOME`, else `~/.config`, then `vinospeak/gui.json`.
pub fn path() -> Option<PathBuf> {
    let config = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|dir| !dir.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|home| Path::new(&home).join(".config")))?;
    Some(config.join("vinospeak").join("gui.json"))
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
    Settings {
        voice: saved
            .get("voice")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
        speed: number("speed"),
        volume: number("volume").filter(|v| (0.0..=1.0).contains(v)),
        translate: saved
            .get("translate")
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned),
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
