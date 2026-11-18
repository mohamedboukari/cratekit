/**
 * Real implementation of CratekitApi: Tauri commands (snake_case, as in
 * src-tauri) and events. Argument keys are camelCase; Tauri maps them to the
 * Rust parameter names. Commands not implemented in Rust yet reject with an
 * error, which callers surface through toAppError().
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWebview } from "@tauri-apps/api/webview";
import { open } from "@tauri-apps/plugin-dialog";

import { AUDIO_EXTENSIONS } from "@/lib/formats";
import type { CratekitApi, Listener, Unlisten } from "@/lib/ipc";

function on<T>(event: string, listener: Listener<T>): Promise<Unlisten> {
  return listen<T>(event, (e) => listener(e.payload));
}

export function createTauriApi(): CratekitApi {
  return {
    appVersion: () => invoke("app_version"),
    appOpenSettings: () => invoke("open_settings_window"),
    appSetMenuLabels: (labels) => invoke("app_set_menu_labels", { labels }),
    dialogOpenFiles: async () =>
      (await open({
        multiple: true,
        directory: false,
        filters: [{ name: "Audio", extensions: AUDIO_EXTENSIONS }],
      })) ?? [],
    dialogOpenFolder: () => open({ multiple: false, directory: true }),

    libraryAddPaths: (paths) => invoke("library_add_paths", { paths }),
    libraryList: (query) => invoke("library_list", { query }),
    libraryCounts: () => invoke("library_counts"),
    trackDetail: (id) => invoke("track_detail", { id }),
    cratesList: () => invoke("crates_list"),
    crateCreate: (name, color) => invoke("crate_create", { name, color }),
    crateAddTracks: (crateId, ids) => invoke("crate_add_tracks", { crateId, ids }),
    crateRemoveTracks: (crateId, ids) => invoke("crate_remove_tracks", { crateId, ids }),
    setlistTransitions: (ids) => invoke("setlist_transitions", { ids }),

    analyzeTracks: (ids) => invoke("analyze_tracks", { ids }),
    jobCancel: (jobId) => invoke("job_cancel", { jobId }),
    jobsSetPaused: (paused) => invoke("jobs_set_paused", { paused }),

    playbackLoad: (id) => invoke("playback_load", { id }),
    playbackPlay: () => invoke("playback_play"),
    playbackPause: () => invoke("playback_pause"),
    playbackSeek: (sec) => invoke("playback_seek", { sec }),
    playbackSetRate: (rate) => invoke("playback_set_rate", { rate }),
    playbackSetMode: (mode) => invoke("playback_set_mode", { mode }),
    playbackSetFilter: (value) => invoke("playback_set_filter", { value }),
    playbackSetLevelMatch: (enabled) => invoke("playback_set_level_match", { enabled }),

    stemsSplit: (id) => invoke("stems_split", { id }),
    stemsSetGain: (stem, gain) => invoke("stems_set_gain", { stem, gain }),
    stemsSetMute: (stem, muted) => invoke("stems_set_mute", { stem, muted }),
    stemsSetSolo: (stem, soloed) => invoke("stems_set_solo", { stem, soloed }),
    stemsBackendStatus: () => invoke("stems_backend_status"),
    stemsInfo: (id) => invoke("stems_info", { id }),
    stemsExport: (id) => invoke("stems_export", { id }),

    masterPresets: () => invoke("master_presets"),
    masterPreview: (id, settings) => invoke("master_preview", { id, settings }),
    masterExport: (id, settings) => invoke("master_export", { id, settings }),

    hotcueSet: (id, slot, sec) => invoke("hotcue_set", { id, slot, sec }),
    hotcueClear: (id, slot) => invoke("hotcue_clear", { id, slot }),
    hotcueUpdate: (id, slot, name, color) =>
      invoke("hotcue_update", { id, slot, name, color }),

    midiListDevices: () => invoke("midi_list_devices"),
    midiLearnStart: (target) => invoke("midi_learn_start", { target }),
    midiLearnCancel: () => invoke("midi_learn_cancel"),
    midiMappings: (deviceId) => invoke("midi_mappings", { deviceId }),

    exportVirtualdj: (ids, dryRun) => invoke("export_virtualdj", { ids, dryRun }),

    modelsStatus: () => invoke("models_status"),
    modelsDownload: (name) => invoke("models_download", { name }),
    modelsDelete: (name) => invoke("models_delete", { name }),

    audioListDevices: () => invoke("audio_list_devices"),
    audioSetDevice: (id) => invoke("audio_set_device", { id }),
    audioSetBufferSize: (frames) => invoke("audio_set_buffer_size", { frames }),

    settingsGet: () => invoke("settings_get"),
    settingsSet: (key, value) => invoke("settings_set", { key, value }),

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
