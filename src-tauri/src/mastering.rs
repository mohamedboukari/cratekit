//! Mastering commands (#24 so far): the presets the Master tab lists. The
//! preview (#26) and the export (#25) come next.

use cratekit_core::mastering::presets::PresetName;
use serde::Serialize;

/// A preset as the Master tab lists it, the UI's `MasterPreset`.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[serde(rename_all = "camelCase")]
pub(crate) struct MasterPresetDto {
    name: PresetName,
    /// English; the UI words known presets itself.
    label: String,
    /// The default loudness target, in LUFS.
    target_lufs: f32,
    /// One English line on the sound.
    description: String,
}

/// Every preset, with its default target (from `PresetName::preset`).
fn presets() -> Vec<MasterPresetDto> {
    PresetName::ALL
        .iter()
        .map(|&name| MasterPresetDto {
            name,
            label: name.label().to_owned(),
            target_lufs: name.preset().target_lufs,
            description: name.description().to_owned(),
        })
        .collect()
}

/// The presets for the Master tab.
#[tauri::command]
pub(crate) fn master_presets() -> Vec<MasterPresetDto> {
    presets()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_every_preset_as_the_ui_reads_it() {
        let json = serde_json::to_value(presets()).expect("serializes");
        assert_eq!(json.as_array().map(Vec::len), Some(5));
        assert_eq!(
            json[0],
            serde_json::json!({
                "name": "club",
                "label": "Club",
                "targetLufs": -8.0,
                "description": "Punchy low end, bright top"
            })
        );
        assert_eq!(json[3]["name"], "lo-fi");
        assert_eq!(json[4]["targetLufs"], -12.0);
    }
}
