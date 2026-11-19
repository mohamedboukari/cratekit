/**
 * Real implementation of CratekitApi: Tauri commands (snake_case, as in
 * src-tauri) and events. Argument keys are camelCase; Tauri maps them to the
 * Rust parameter names. Commands not implemented in Rust yet reject with the
 * `not_built` code, which notifyError() shows as a calm "not built yet" note.
 */
import { invoke, type InvokeArgs } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

import { AUDIO_EXTENSIONS } from "@/lib/formats";
import type { CratekitApi, Listener, Unlisten } from "@/lib/ipc";

/** Tauri's rejection text when no Rust command has this name. */
const MISSING_COMMAND = /^Command (\S+) not found$/;

/**
 * Tauri's invoke(), except that a command Rust does not have yet rejects with
 * `{ code: "not_built" }` instead of Tauri's raw text, so the UI can tell
 * "planned for a later version" apart from a real failure.
 */
export async function call<T>(command: string, args?: InvokeArgs): Promise<T> {
  try {
    return await invoke<T>(command, args);
  } catch (error) {
    if (typeof error === "string") {
      const missing = MISSING_COMMAND.exec(error);
      if (missing) throw { code: "not_built", message: missing[1] };
    }
    throw error;
  }
}

function on<T>(event: string, listener: Listener<T>): Promise<Unlisten> {
  return listen<T>(event, (e) => listener(e.payload));
}

export function createTauriApi(): CratekitApi {
  return {
    appVersion: () => call("app_version"),
    appOpenSettings: () => call("open_settings_window"),
    appSetMenuLabels: (labels) => call("app_set_menu_labels", { labels }),
    dialogOpenFiles: async () =>
      (await open({
        multiple: true,
        directory: false,
        filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }],
      })) ?? [],
    dialogOpenFolder: () => open({ multiple: false, directory: true }),

    libraryAddPaths: (paths) => call("library_add_paths", { paths }),
    libraryList: (query) => call("library_list", { query }),
    libraryCounts: () => call("library_counts"),
    trackDetail: (id) => call("track_detail", { id }),
    cratesList: () => call("crates_list"),
    crateCreate: (name, color) => call("crate_create", { name, color }),
    crateAddTracks: (crateId, ids) => call("crate_add_tracks", { crateId, ids }),
    crateRemoveTracks: (crateId, ids) => call("crate_remove_tracks", { crateId, ids }),
    setlistTransitions: (ids) => call("setlist_transitions", { ids }),

    analyzeTracks: (ids) => call("analyze_tracks", { ids }),
    jobCancel: (jobId) => call("job_cancel", { jobId }),
    jobsSetPaused: (paused) => call("jobs_set_paused", { paused }),

    playbackLoad: (id) => call("playback_load", { id }),
    playbackPlay: () => call("playback_play"),
    playbackPause: () => call("playback_pause"),
    playbackSeek: (sec) => call("playback_seek", { sec }),
    playbackSetRate: (rate) => call("playback_set_rate", { rate }),
    playbackSetMode: (mode) => call("playback_set_mode", { mode }),
    playbackSetFilter: (value) => call("playback_set_filter", { value }),
    playbackSetLevelMatch: (enabled) => call("playback_set_level_match", { enabled }),

    stemsSplit: (id) => call("stems_split", { id }),
    stemsSetGain: (stem, gain) => call("stems_set_gain", { stem, gain }),
    stemsSetMute: (stem, muted) => call("stems_set_mute", { stem, muted }),
    stemsSetSolo: (stem, soloed) => call("stems_set_solo", { stem, soloed }),
    stemsBackendStatus: () => call("stems_backend_status"),
    stemsInfo: (id) => call("stems_info", { id }),
    stemsExport: (id) => call("stems_export", { id }),

    masterPresets: () => call("master_presets"),
    masterPreview: (id, settings) => call("master_preview", { id, settings }),
    masterExport: (id, settings) => call("master_export", { id, settings }),

    hotcueSet: (id, slot, sec) => call("hotcue_set", { id, slot, sec }),
    hotcueClear: (id, slot) => call("hotcue_clear", { id, slot }),
    hotcueUpdate: (id, slot, name, color) =>
      call("hotcue_update", { id, slot, name, color }),

    midiListDevices: () => call("midi_list_devices"),
    midiLearnStart: (target) => call("midi_learn_start", { target }),
    midiLearnCancel: () => call("midi_learn_cancel"),
    midiMappings: (deviceId) => call("midi_mappings", { deviceId }),

    exportVirtualdj: (ids, dryRun) => call("export_virtualdj", { ids, dryRun }),

    modelsStatus: () => call("models_status"),
    modelsDownload: (name) => call("models_download", { name }),
    modelsDelete: (name) => call("models_delete", { name }),

    audioListDevices: () => call("audio_list_devices"),
    audioSetDevice: (id) => call("audio_set_device", { id }),

    settingsGet: () => call("settings_get"),
    settingsSet: (key, value) => call("settings_set", { key, value }),

    onJobProgress: (l) => on("job_progress", l),
    onJobDone: (l) => on("job_done", l),
    onJobFailed: (l) => on("job_failed", l),
    onPlaybackPosition: (l) => on<{ sec: number }>("playback_position", (p) => l(p.sec)),
    onMeter: (l) => on("meter", l),
    onPlaybackState: (l) => on("playback_state", l),
    onMidiDeviceChanged: (l) => on("midi_device_changed", l),
    onMidiLearned: (l) => on("midi_learned", l),
    onControlEvent: (l) => on("control_event", l),
    onAudioDeviceChanged: (l) => on("audio_device_changed", l),
    onFilesDropped: (l) => {
      // Only drags that carry files from the OS; in-app drags of tracks report no paths.
      let carriesFiles = false;
      return getCurrentWebview().onDragDropEvent((e) => {
        const p = e.payload;
        if (p.type === "enter") {
          carriesFiles = p.paths.length > 0;
          if (carriesFiles) l({ kind: "enter", paths: p.paths });
        } else if (!carriesFiles) {
          return;
        } else if (p.type === "drop") {
          carriesFiles = false;
          l({ kind: "drop", paths: p.paths });
        } else if (p.type === "leave") {
          carriesFiles = false;
          l({ kind: "leave", paths: [] });
        } else l({ kind: "over", paths: [] });
      });
    },
    onSettingsChanged: (l) => on("settings_changed", l),
  };
}
