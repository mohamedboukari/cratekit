/**
 * Payload types shared by the UI and the Rust backend. They mirror the IPC
 * tables and the SQLite schema in docs/ARCHITECTURE.md; keep both in sync.
 * Rust remains the source of truth for every value (the UI never computes
 * analysis results, matches or loudness itself).
 */

export type TrackId = number;
export type CrateId = number;
export type JobId = string;

/** Every Rust command rejects with this shape (AppError in src-tauri/src/lib.rs). */
export interface AppError {
  code: string;
  message: string;
}

/** One row of the track browser. */
export interface TrackRow {
  id: TrackId;
  title: string;
  artist: string;
  durationSec: number;
  bpm: number | null;
  /** Camelot code, e.g. "8A". */
  camelot: string | null;
  /** Human key name, e.g. "A minor". */
  keyName: string | null;
  /** 1 to 10. */
  energy: number | null;
  lufs: number | null;
  analyzed: boolean;
  hasStems: boolean;
  mastered: boolean;
  /** Set while a job runs on this track. */
  activity: TrackActivity | null;
}

export interface TrackActivity {
  kind: JobKind;
  /** 0 to 1. */
  progress: number;
}

export type CueSlot = 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8;

export type CueColor =
  "orange" | "blue" | "pink" | "purple" | "lime" | "yellow" | "red" | "teal";

export interface Cue {
  slot: CueSlot;
  positionSec: number;
  color: CueColor;
  name: string | null;
}

export interface Analysis {
  /** Null for a track with no steady beat. */
  bpm: number | null;
  /** 0 to 1. */
  bpmConfidence: number;
  /** Null for a track with no notes. */
  keyName: string | null;
  camelot: string | null;
  /** 0 to 1. */
  keyConfidence: number;
  /** 1 to 10; null for silence. */
  energy: number | null;
  /** Null for silence. */
  lufs: number | null;
  truePeakDb: number | null;
  /** The first beat (with `bpm`, the beat grid); null without a beat. */
  firstBeatSec: number | null;
  /** 12 values, C to B, 0 to 1. */
  chroma: number[];
  /** 12 flags, C to B: the pitch classes of the detected key's scale (computed in Rust). */
  chromaInKey: boolean[];
  /** Camelot codes that mix well with this track (computed in Rust). */
  mixesWellWith: string[];
  dropSec: number | null;
  mixOutSec: number | null;
}

/** Waveform overview: one column per pixel bucket, values 0 to 1. */
export interface Waveform {
  peaks: number[];
  rms: number[];
}

export interface TrackDetail {
  track: TrackRow;
  analysis: Analysis | null;
  cues: Cue[];
  waveform: Waveform | null;
}

export interface Crate {
  id: CrateId;
  name: string;
  color: CueColor;
  trackCount: number;
}

export interface LibraryCounts {
  all: number;
  recentlyAdded: number;
  needsAnalysis: number;
}

export interface LibraryQuery {
  crateId: CrateId | null;
  search: string;
  bpmMin: number | null;
  bpmMax: number | null;
  /** Only tracks within mixing range of this BPM (tolerance decided in Rust). */
  nearBpm: number | null;
  /** Only tracks that mix well with this Camelot code (matching done in Rust). */
  matchCamelot: string | null;
  hasStems: boolean;
}

export type JobKind = "analysis" | "stems" | "master" | "export" | "model-download";

export type JobState = "queued" | "running" | "paused" | "done" | "failed";

/** A step inside a multi-step job, as a code the UI translates (Rust stays language-neutral). */
export type JobStage = "download" | "verify";

export interface Job {
  id: JobId;
  kind: JobKind;
  /** Rust's own label, in English: a fallback only; the UI shows `jobLabel(job)`. */
  label: string;
  /** The current step of a multi-step job (the model download), or null. */
  stage: JobStage | null;
  /** Tracks this job works on (empty for jobs such as a model download). */
  trackIds: TrackId[];
  done: number;
  total: number;
  state: JobState;
}

export interface JobProgress {
  jobId: JobId;
  kind: JobKind;
  /** Rust's own label, in English: a fallback only; the UI translates by kind and stage. */
  label: string;
  /** The current step of a multi-step job (the model download), or null. */
  stage: JobStage | null;
  /** Tracks this job works on (empty for jobs such as a model download). */
  trackIds: TrackId[];
  /** Progress steps, not tracks (Rust's analysis counts 100 per track). */
  done: number;
  total: number;
  /** Waiting for an earlier job or a resume; not started yet. */
  queued: boolean;
}

export interface JobDone {
  jobId: JobId;
  kind: JobKind;
}

export interface JobFailed {
  jobId: JobId;
  kind: JobKind;
  error: AppError;
}

/** Sent when playback starts, pauses or reaches the end of the track. */
export interface PlaybackState {
  playing: boolean;
}

export type PlaybackMode = "original" | "mastered" | "stems";

export type StemName = "vocals" | "drums" | "bass" | "other";

export interface MeterReading {
  /** dBFS, 0 is full scale. */
  peakDb: number;
  rmsDb: number;
}

export type PresetName = "club" | "hip-hop" | "pop" | "lo-fi" | "chill";

export interface MasterSettings {
  preset: PresetName;
  /** 0 to 1. */
  intensity: number;
  targetLufs: number;
  referenceTrackId: TrackId | null;
  bitDepth: 16 | 24;
}

/** A mastering preset as Rust defines it (parameters in docs/ARCHITECTURE.md). */
export interface MasterPreset {
  name: PresetName;
  label: string;
  /** Default loudness target for this preset. */
  targetLufs: number;
  /** One line for the UI, e.g. "Punchy low end, bright top". */
  description: string;
}

export interface MasterResult {
  path: string;
  lufs: number;
  truePeakDb: number;
}

export type ControlTarget =
  | "play"
  | "cue"
  | `hotcue-${CueSlot}`
  | `mute-${StemName}`
  | "filter"
  | "tempo"
  | "volume"
  | "ab-toggle"
  | "pad-page";

export interface MidiDevice {
  id: string;
  name: string;
  connected: boolean;
}

export interface MidiBinding {
  kind: "note" | "cc";
  channel: number;
  number: number;
}

export interface MidiMapping {
  deviceId: string;
  target: ControlTarget;
  binding: MidiBinding | null;
}

export interface MidiLearned {
  target: ControlTarget;
  binding: MidiBinding;
  /** The target that used this control before; Rust moved the binding here. */
  replaced: ControlTarget | null;
}

export interface ControlEvent {
  target: ControlTarget;
  /** 0 to 1 for continuous controls, 1 for a button press. */
  value: number;
}

export interface AudioDevice {
  id: string;
  name: string;
  isDefault: boolean;
  sampleRate: number;
}

export interface AudioDeviceChanged {
  /** The device now in use; for "lost", the one that failed; for "list-changed", the default. */
  id: string;
  /**
   * selected: picked by the user, or the picked device came back. unplugged:
   * the device in use failed or disappeared, playback moved to the default.
   * default-changed: the app follows the system default and it changed.
   * lost: the output failed and no other could open. list-changed: devices
   * were plugged or unplugged (reload the list only).
   */
  reason: "selected" | "unplugged" | "default-changed" | "lost" | "list-changed";
}

/** Split stems of one track (null from `stems_info` when not split yet). */
export interface StemsInfo {
  /** How long the split took, in seconds. */
  splitSec: number;
  waveforms: Record<StemName, Waveform>;
  /** Drums, bass and other summed (the 2-stem layout), computed in Rust. */
  instrumental: Waveform;
}

export type StemsBackendPreference = "auto" | "cpu";

/** Why stems run on the CPU: CPU only is chosen in Settings, or this version splits on the CPU. */
export type StemsCpuReason = "cpu_chosen" | "gpu_unsupported";

export interface StemsBackendStatus {
  active: "coreml" | "directml" | "cpu";
  /** Why the CPU is used, as a code the UI words; null when a GPU is in use. */
  reason: StemsCpuReason | null;
}

export interface ModelStatus {
  name: string;
  sizeBytes: number;
  downloaded: boolean;
}

/** How two neighbouring tracks in the set list fit together (computed in Rust). */
export interface Transition {
  fromId: TrackId;
  toId: TrackId;
  /** Same key or a neighbour on the Camelot wheel; null when either key is unknown. */
  keyCompatible: boolean | null;
  /** BPM change in percent from `fromId` to `toId`; null when either is not analyzed. */
  bpmChangePct: number | null;
  /** The BPM change is too big to mix smoothly (threshold decided in Rust). */
  bpmJump: boolean;
}

/** Files dragged from the OS onto the window (paths only on `enter` and `drop`). */
export interface FileDrop {
  kind: "enter" | "over" | "leave" | "drop";
  paths: string[];
}

/** Saved pane sizes, per panel group id: panel id to percentage. */
export type PaneLayouts = Record<string, Record<string, number>>;

/** UI languages (#80). English is the default for every user. */
export type UiLanguage = "en" | "fr";

/** Text of every native menu item and the Settings window title, in the UI language. */
export interface MenuLabels {
  settings: string;
  settingsWindowTitle: string;
  file: string;
  edit: string;
  window: string;
  undo: string;
  redo: string;
  cut: string;
  copy: string;
  paste: string;
  selectAll: string;
  minimize: string;
  closeWindow: string;
  quit: string;
  about: string;
  services: string;
  hide: string;
  hideOthers: string;
  showAll: string;
}

export interface Settings {
  /** The UI language, chosen in Settings (never taken from the system). */
  language: UiLanguage;
  audioDeviceId: string | null;
  bufferSize: number;
  watchedFolders: string[];
  watchFolders: boolean;
  outputFolder: string;
  stemsBackend: StemsBackendPreference;
  paneLayouts: PaneLayouts;
  /** The set builder side list, in order. */
  setList: TrackId[];
}

/** One value the export writes for a track: data, not text (the UI words it). */
export type ExportChangeItem =
  | { field: "bpm"; value: number | null }
  | { field: "key"; value: string | null }
  | { field: "cues"; value: number };

export interface ExportChange {
  trackId: TrackId;
  title: string;
  changes: ExportChangeItem[];
}

export interface ExportPlan {
  dryRun: boolean;
  backupPath: string;
  tracks: ExportChange[];
}
