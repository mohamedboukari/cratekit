/**
 * Real implementation of CratekitApi: Tauri commands (snake_case, as in
 * src-tauri) and events. Argument keys are camelCase; Tauri maps them to the
 * Rust parameter names. Commands not implemented in Rust yet reject with an
 * error, which callers surface through toAppError().
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import type { CratekitApi, Listener, Unlisten } from "@/lib/ipc";

function on<T>(event: string, listener: Listener<T>): Promise<Unlisten> {
  return listen<T>(event, (e) => listener(e.payload));
}

export function createTauriApi(): CratekitApi {
  return {
    appVersion: () => invoke("app_version"),

    libraryAddPaths: (paths) => invoke("library_add_paths", { paths }),
    libraryList: (query) => invoke("library_list", { query }),
    libraryCounts: () => invoke("library_counts"),
    trackDetail: (id) => invoke("track_detail", { id }),
    cratesList: () => invoke("crates_list"),
    crateCreate: (name, color) => invoke("crate_create", { name, color }),
    crateAddTracks: (crateId, ids) => invoke("crate_add_tracks", { crateId, ids }),
    crateRemoveTracks: (crateId, ids) => invoke("crate_remove_tracks", { crateId, ids }),

    analyzeTracks: (ids) => invoke("analyze_tracks", { ids }),
    jobCancel: (jobId) => invoke("job_cancel", { jobId }),
    jobsSetPaused: (paused) => invoke("jobs_set_paused", { paused }),

    playbackLoad: (id) => invoke("playback_load", { id }),
    playbackPlay: () => invoke("playback_play"),
    playbackPause: () => invoke("playback_pause"),
    playbackSeek: (sec) => invoke("playback_seek", { sec }),
    playbackSetRate: (rate) => invoke("playback_set_rate", { rate }),
    playbackSetMode: (mode) => invoke("playback_set_mode", { mode }),

    stemsSplit: (id) => invoke("stems_split", { id }),
    stemsSetGain: (stem, gain) => invoke("stems_set_gain", { stem, gain }),
    stemsSetMute: (stem, muted) => invoke("stems_set_mute", { stem, muted }),
    stemsBackendStatus: () => invoke("stems_backend_status"),

    masterPreview: (id, settings) => invoke("master_preview", { id, settings }),
    masterExport: (id, settings, path) => invoke("master_export", { id, settings, path }),

    hotcueSet: (id, slot, sec) => invoke("hotcue_set", { id, slot, sec }),
    hotcueClear: (id, slot) => invoke("hotcue_clear", { id, slot }),

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
  };
}
