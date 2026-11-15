/**
 * The only module that talks to Rust. Components, hooks and stores call
 * `api()` and never use `invoke()` or `listen()` themselves.
 *
 * Two implementations of the same contract:
 * - `ipc.tauri.ts`: real Tauri commands and events (inside the desktop app).
 * - `@/mock/api`: deterministic fake data (browser dev server and the
 *   GitHub Pages demo). It is loaded only when `import.meta.env.DEV` or
 *   `VITE_MOCK=1`, so production Tauri builds never contain it.
 *
 * Commands reject with `AppError` (`{ code, message }`).
 */
import { isTauri } from "@tauri-apps/api/core";

import type {
  AppError,
  AudioDevice,
  AudioDeviceChanged,
  ControlEvent,
  ControlTarget,
  Crate,
  CrateId,
  CueColor,
  CueSlot,
  ExportPlan,
  JobDone,
  JobFailed,
  JobId,
  JobProgress,
  LibraryCounts,
  LibraryQuery,
  MasterResult,
  MasterSettings,
  MeterReading,
  MidiDevice,
  MidiLearned,
  MidiMapping,
  ModelStatus,
  PlaybackMode,
  PlaybackState,
  Settings,
  StemName,
  StemsBackendStatus,
  TrackDetail,
  TrackId,
  TrackRow,
  Transition,
} from "@/lib/types";

/** Stops an event subscription. */
export type Unlisten = () => void;

export type Listener<T> = (payload: T) => void;

/** Every Rust command and event, typed. Mirrors the IPC tables in docs/ARCHITECTURE.md. */
export interface CratekitApi {
  // App
  appVersion(): Promise<string>;

  // Library and crates
  libraryAddPaths(paths: string[]): Promise<TrackId[]>;
  libraryList(query: LibraryQuery): Promise<TrackRow[]>;
  libraryCounts(): Promise<LibraryCounts>;
  trackDetail(id: TrackId): Promise<TrackDetail>;
  cratesList(): Promise<Crate[]>;
  crateCreate(name: string, color: CueColor): Promise<Crate>;
  crateAddTracks(crateId: CrateId, ids: TrackId[]): Promise<void>;
  crateRemoveTracks(crateId: CrateId, ids: TrackId[]): Promise<void>;
  /** Key and BPM fit between each pair of neighbours in a set list. */
  setlistTransitions(ids: TrackId[]): Promise<Transition[]>;

  // Jobs
  analyzeTracks(ids: TrackId[]): Promise<JobId>;
  jobCancel(jobId: JobId): Promise<void>;
  jobsSetPaused(paused: boolean): Promise<void>;

  // Playback
  playbackLoad(id: TrackId): Promise<void>;
  playbackPlay(): Promise<void>;
  playbackPause(): Promise<void>;
  playbackSeek(sec: number): Promise<void>;
  playbackSetRate(rate: number): Promise<void>;
  playbackSetMode(mode: PlaybackMode): Promise<void>;
  /** DJ filter: -1 full low-pass, 0 off, +1 full high-pass. */
  playbackSetFilter(value: number): Promise<void>;

  // Stems
  stemsSplit(id: TrackId): Promise<JobId>;
  stemsSetGain(stem: StemName, gain: number): Promise<void>;
  stemsSetMute(stem: StemName, muted: boolean): Promise<void>;
  stemsBackendStatus(): Promise<StemsBackendStatus>;

  // Mastering
  masterPreview(id: TrackId, settings: MasterSettings): Promise<void>;
  masterExport(
    id: TrackId,
    settings: MasterSettings,
    path: string,
  ): Promise<MasterResult>;

  // Cues
  hotcueSet(id: TrackId, slot: CueSlot, sec: number): Promise<void>;
  hotcueClear(id: TrackId, slot: CueSlot): Promise<void>;

  // MIDI
  midiListDevices(): Promise<MidiDevice[]>;
  midiLearnStart(target: ControlTarget): Promise<void>;
  midiLearnCancel(): Promise<void>;
  midiMappings(deviceId: string): Promise<MidiMapping[]>;

  // Export
  exportVirtualdj(ids: TrackId[], dryRun: boolean): Promise<ExportPlan>;

  // Models
  modelsStatus(): Promise<ModelStatus[]>;
  modelsDownload(name: string): Promise<JobId>;
  modelsDelete(name: string): Promise<void>;

  // Audio output
  audioListDevices(): Promise<AudioDevice[]>;
  audioSetDevice(id: string): Promise<void>;
  audioSetBufferSize(frames: number): Promise<void>;

  // Settings
  settingsGet(): Promise<Settings>;
  settingsSet<K extends keyof Settings>(key: K, value: Settings[K]): Promise<void>;

  // Events (Rust to UI)
  onJobProgress(listener: Listener<JobProgress>): Promise<Unlisten>;
  onJobDone(listener: Listener<JobDone>): Promise<Unlisten>;
  onJobFailed(listener: Listener<JobFailed>): Promise<Unlisten>;
  /** About 30 Hz while playing: write into a ref, never into React state. */
  onPlaybackPosition(listener: Listener<number>): Promise<Unlisten>;
  /** About 30 Hz while playing: write into a ref, never into React state. */
  onMeter(listener: Listener<MeterReading>): Promise<Unlisten>;
  /** Human-speed: playback started, paused, or the track ended. */
  onPlaybackState(listener: Listener<PlaybackState>): Promise<Unlisten>;
  onMidiDeviceChanged(listener: Listener<MidiDevice[]>): Promise<Unlisten>;
  onMidiLearned(listener: Listener<MidiLearned>): Promise<Unlisten>;
  onControlEvent(listener: Listener<ControlEvent>): Promise<Unlisten>;
  onAudioDeviceChanged(listener: Listener<AudioDeviceChanged>): Promise<Unlisten>;
}

/** True when the fake-data implementation is in use (browser and demo). */
export function isMockMode(): boolean {
  return mockMode;
}

let current: CratekitApi | null = null;
let mockMode = false;

/**
 * Picks and loads the implementation once. Call before rendering (main.tsx).
 * The mock branch is removed from production builds unless VITE_MOCK=1.
 */
export async function initApi(): Promise<CratekitApi> {
  if (current) return current;
  const mockAllowed = import.meta.env.DEV || import.meta.env.VITE_MOCK === "1";
  if (mockAllowed && (import.meta.env.VITE_MOCK === "1" || !isTauri())) {
    const { createMockApi } = await import("@/mock/api");
    current = createMockApi();
    mockMode = true;
  } else {
    const { createTauriApi } = await import("@/lib/ipc.tauri");
    current = createTauriApi();
  }
  return current;
}

/** The active implementation. Throws if `initApi()` has not run. */
export function api(): CratekitApi {
  if (!current) {
    throw new Error("Cratekit: initApi() must run before api() is used");
  }
  return current;
}

/** Tests only: install an implementation directly (usually the mock). */
export function setApiForTests(impl: CratekitApi, mock = true): void {
  current = impl;
  mockMode = mock;
}

/** Normalizes anything a command rejects with into an AppError. */
export function toAppError(error: unknown): AppError {
  if (
    typeof error === "object" &&
    error !== null &&
    "code" in error &&
    "message" in error
  ) {
    const { code, message } = error;
    return { code: String(code), message: String(message) };
  }
  if (error instanceof Error) return { code: "unknown", message: error.message };
  return { code: "unknown", message: String(error) };
}
