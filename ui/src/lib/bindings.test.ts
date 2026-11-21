/**
 * The IPC contract, checked by the type checker. `bindings/` holds the
 * TypeScript types ts-rs generates from the Rust structs (`cargo nextest run
 * --all-features` writes them; never edit them by hand). The UI keeps its own
 * types in `types.ts`, because it is ahead of Rust in places (stems,
 * mastering, live job activity). These checks prove the two agree in the
 * direction the data flows:
 *
 * - Rust to UI (events and command results): what Rust sends fits the UI type.
 * - UI to Rust (command arguments): what the UI sends fits the Rust type.
 *
 * A renamed, removed or retyped Rust field fails `bun run typecheck`.
 */
import { describe, expectTypeOf, it } from "vitest";

import type { Analysis as RustAnalysis } from "@/lib/bindings/Analysis";
import type { AppError as RustAppError } from "@/lib/bindings/AppError";
import type { AudioDeviceDto } from "@/lib/bindings/AudioDeviceDto";
import type { Crate as RustCrate } from "@/lib/bindings/Crate";
import type { Cue as RustCue } from "@/lib/bindings/Cue";
import type { CueColor as RustCueColor } from "@/lib/bindings/CueColor";
import type { DeviceChanged } from "@/lib/bindings/DeviceChanged";
import type { JobDone as RustJobDone } from "@/lib/bindings/JobDone";
import type { JobFailed as RustJobFailed } from "@/lib/bindings/JobFailed";
import type { JobProgress as RustJobProgress } from "@/lib/bindings/JobProgress";
import type { LibraryCounts as RustLibraryCounts } from "@/lib/bindings/LibraryCounts";
import type { LibraryQuery as RustLibraryQuery } from "@/lib/bindings/LibraryQuery";
import type { MenuLabels as RustMenuLabels } from "@/lib/bindings/MenuLabels";
import type { Meter } from "@/lib/bindings/Meter";
import type { ModelStatus as RustModelStatus } from "@/lib/bindings/ModelStatus";
import type { PlaybackState as RustPlaybackState } from "@/lib/bindings/PlaybackState";
import type { Position } from "@/lib/bindings/Position";
import type { Settings as RustSettings } from "@/lib/bindings/Settings";
import type { StemsInfoDto } from "@/lib/bindings/StemsInfoDto";
import type { TrackDetailDto } from "@/lib/bindings/TrackDetailDto";
import type { TrackRow as RustTrackRow } from "@/lib/bindings/TrackRow";
import type { TrackRowDto } from "@/lib/bindings/TrackRowDto";
import type { Transition as RustTransition } from "@/lib/bindings/Transition";
import type { WaveformView } from "@/lib/bindings/WaveformView";
import type {
  Analysis,
  AppError,
  AudioDevice,
  AudioDeviceChanged,
  Crate,
  Cue,
  CueColor,
  JobDone,
  JobFailed,
  JobProgress,
  LibraryCounts,
  LibraryQuery,
  MenuLabels,
  MeterReading,
  ModelStatus,
  PlaybackState,
  Settings,
  StemsInfo,
  TrackDetail,
  TrackRow,
  Transition,
  Waveform,
} from "@/lib/types";

describe("IPC contract: Rust payloads fit the UI types", () => {
  it("library", () => {
    expectTypeOf<RustTrackRow>().toExtend<Omit<TrackRow, "activity">>();
    expectTypeOf<TrackRowDto>().toExtend<TrackRow>();
    expectTypeOf<TrackDetailDto>().toExtend<TrackDetail>();
    expectTypeOf<RustAnalysis>().toExtend<Analysis>();
    expectTypeOf<RustCue>().toExtend<Cue>();
    expectTypeOf<RustCueColor>().toExtend<CueColor>();
    expectTypeOf<RustCrate>().toExtend<Crate>();
    expectTypeOf<RustLibraryCounts>().toExtend<LibraryCounts>();
    expectTypeOf<RustTransition>().toExtend<Transition>();
    expectTypeOf<WaveformView>().toExtend<Waveform>();
  });

  it("jobs and errors", () => {
    expectTypeOf<RustJobProgress>().toExtend<JobProgress>();
    expectTypeOf<RustJobDone>().toExtend<JobDone>();
    expectTypeOf<RustJobFailed>().toExtend<JobFailed>();
    expectTypeOf<RustAppError>().toExtend<AppError>();
  });

  it("models and stems", () => {
    expectTypeOf<RustModelStatus>().toExtend<ModelStatus>();
    expectTypeOf<StemsInfoDto>().toExtend<StemsInfo>();
  });

  it("playback and audio devices", () => {
    expectTypeOf<Position>().toExtend<{ sec: number }>();
    expectTypeOf<Meter>().toExtend<MeterReading>();
    expectTypeOf<RustPlaybackState>().toExtend<PlaybackState>();
    expectTypeOf<DeviceChanged>().toExtend<AudioDeviceChanged>();
    expectTypeOf<AudioDeviceDto>().toExtend<AudioDevice>();
  });

  it("settings", () => {
    expectTypeOf<RustSettings>().toExtend<Settings>();
  });
});

describe("IPC contract: UI arguments fit the Rust types", () => {
  it("command arguments and settings_set values", () => {
    expectTypeOf<LibraryQuery>().toExtend<RustLibraryQuery>();
    expectTypeOf<CueColor>().toExtend<RustCueColor>();
    expectTypeOf<Settings>().toExtend<RustSettings>();
    expectTypeOf<MenuLabels>().toExtend<RustMenuLabels>();
  });
});
