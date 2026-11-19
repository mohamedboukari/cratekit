//! App settings (SPEC, Module 7): their types, defaults and rules. They are
//! stored in the library database (`Library::settings`,
//! `Library::set_setting`), one JSON value per key, so they survive
//! restarts. See docs/settings.md.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::CoreError;
use crate::library::TrackId;

/// Audio buffer sizes the Settings screen offers, in frames.
pub const BUFFER_SIZES: [u32; 4] = [128, 256, 512, 1024];

/// The UI language. English for everyone until the user picks another.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum Language {
    /// English, the default.
    #[default]
    En,
    /// French.
    Fr,
}

/// Where stems run: the GPU when one is supported, or always the CPU.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "lowercase")]
pub enum StemsBackend {
    /// The GPU when supported, else the CPU.
    #[default]
    Auto,
    /// Always the CPU.
    Cpu,
}

/// Every setting (the UI's `Settings`). Missing fields take their default,
/// so a database from an older version still loads.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// The UI language.
    pub language: Language,
    /// The chosen output device, or `None` for the system default.
    pub audio_device_id: Option<String>,
    /// Audio buffer size in frames (one of [`BUFFER_SIZES`]).
    pub buffer_size: u32,
    /// Folders the user added, to watch for new files.
    pub watched_folders: Vec<String>,
    /// Whether to add new files from the watched folders automatically.
    pub watch_folders: bool,
    /// Where stems, masters and exports are written; empty means the app's
    /// default folder.
    pub output_folder: String,
    /// Where stems run.
    pub stems_backend: StemsBackend,
    /// Sizes of the resizable panes, by group, as percentages.
    pub pane_layouts: BTreeMap<String, BTreeMap<String, f64>>,
    /// The set builder side list, in order.
    pub set_list: Vec<TrackId>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: Language::default(),
            audio_device_id: None,
            buffer_size: 512,
            watched_folders: Vec::new(),
            watch_folders: false,
            output_folder: String::new(),
            stems_backend: StemsBackend::default(),
            pane_layouts: BTreeMap::new(),
            set_list: Vec::new(),
        }
    }
}

impl Settings {
    /// The settings with `key` (its camelCase name, as the UI sends it) set
    /// to `value`, checked: the key must exist, the value must have its type,
    /// and the result must follow the rules (see [`Settings::check`]).
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidSetting`] for an unknown key, a value of the wrong
    /// type, or one the rules reject.
    pub fn with(&self, key: &str, value: Value) -> Result<Self, CoreError> {
        let invalid = |reason: String| CoreError::InvalidSetting {
            key: key.to_owned(),
            reason,
        };
        let Value::Object(mut fields) =
            serde_json::to_value(self).map_err(|err| invalid(err.to_string()))?
        else {
            return Err(invalid("settings are not an object".into()));
        };
        if !fields.contains_key(key) {
            return Err(invalid("there is no such setting".into()));
        }
        fields.insert(key.to_owned(), value);
        let updated: Self = serde_json::from_value(Value::Object(fields))
            .map_err(|err| invalid(err.to_string()))?;
        updated.check().map_err(|reason| invalid(reason.into()))?;
        Ok(updated)
    }

    /// The rules a value's type cannot express.
    fn check(&self) -> Result<(), &'static str> {
        if !BUFFER_SIZES.contains(&self.buffer_size) {
            return Err("the buffer size must be 128, 256, 512 or 1024 frames");
        }
        Ok(())
    }

    /// Settings from stored `(key, JSON text)` pairs over the defaults. A
    /// value that no longer parses or breaks a rule (written by another
    /// version, or damaged) is skipped: that setting keeps its default and
    /// the rest still load.
    #[must_use]
    pub fn from_stored(pairs: impl IntoIterator<Item = (String, String)>) -> Self {
        let mut settings = Self::default();
        for (key, text) in pairs {
            let Ok(value) = serde_json::from_str::<Value>(&text) else {
                continue;
            };
            if let Ok(updated) = settings.with(&key, value) {
                settings = updated;
            }
        }
        settings
    }

    /// The stored JSON text of `key`.
    ///
    /// # Errors
    ///
    /// [`CoreError::InvalidSetting`] for an unknown key.
    pub fn stored_value(&self, key: &str) -> Result<String, CoreError> {
        let invalid = |reason: String| CoreError::InvalidSetting {
            key: key.to_owned(),
            reason,
        };
        let value = serde_json::to_value(self).map_err(|err| invalid(err.to_string()))?;
        let field = value
            .get(key)
            .ok_or_else(|| invalid("there is no such setting".into()))?;
        Ok(field.to_string())
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn defaults_match_the_ui_defaults() {
        assert_eq!(
            serde_json::to_value(Settings::default()).unwrap(),
            json!({
                "language": "en", "audioDeviceId": null, "bufferSize": 512,
                "watchedFolders": [], "watchFolders": false, "outputFolder": "",
                "stemsBackend": "auto", "paneLayouts": {}, "setList": []
            })
        );
    }

    #[test]
    fn sets_a_value_of_the_right_type() {
        let settings = Settings::default()
            .with("language", json!("fr"))
            .unwrap()
            .with("bufferSize", json!(256))
            .unwrap()
            .with(
                "paneLayouts",
                json!({ "main": { "browser": 60.5, "tools": 39.5 } }),
            )
            .unwrap()
            .with("setList", json!([3, 1, 2]))
            .unwrap()
            .with("audioDeviceId", json!("usb-1"))
            .unwrap();
        assert_eq!(settings.language, Language::Fr);
        assert_eq!(settings.buffer_size, 256);
        assert_eq!(settings.pane_layouts["main"]["browser"], 60.5);
        assert_eq!(settings.set_list, [3, 1, 2]);
        assert_eq!(settings.audio_device_id.as_deref(), Some("usb-1"));
        let back = settings.with("audioDeviceId", json!(null)).unwrap();
        assert_eq!(back.audio_device_id, None, "back to the system default");
    }

    #[test]
    fn rejects_unknown_keys_wrong_types_and_broken_rules() {
        let settings = Settings::default();
        for (key, value) in [
            ("volume", json!(1)),
            ("language", json!("de")),
            ("bufferSize", json!("512")),
            ("bufferSize", json!(300)),
            ("watchFolders", json!("yes")),
            ("setList", json!(["a"])),
        ] {
            let result = settings.with(key, value.clone());
            assert!(
                matches!(&result, Err(CoreError::InvalidSetting { key: k, .. }) if k == key),
                "{key} = {value}: {result:?}"
            );
        }
    }

    #[test]
    fn stored_values_load_over_the_defaults_and_bad_ones_are_skipped() {
        let settings = Settings::from_stored([
            ("language".to_owned(), "\"fr\"".to_owned()),
            ("bufferSize".to_owned(), "1024".to_owned()),
            ("outputFolder".to_owned(), "not json".to_owned()),
            ("watchFolders".to_owned(), "\"maybe\"".to_owned()),
            ("removedSetting".to_owned(), "1".to_owned()),
        ]);
        assert_eq!(settings.language, Language::Fr);
        assert_eq!(settings.buffer_size, 1024);
        assert_eq!(settings.output_folder, "");
        assert!(!settings.watch_folders);
    }

    #[test]
    fn a_stored_value_round_trips() {
        let settings = Settings::default().with("setList", json!([7, 8])).unwrap();
        let text = settings.stored_value("setList").unwrap();
        assert_eq!(text, "[7,8]");
        let loaded = Settings::from_stored([("setList".to_owned(), text)]);
        assert_eq!(loaded.set_list, [7, 8]);
        assert!(settings.stored_value("nope").is_err());
    }
}
