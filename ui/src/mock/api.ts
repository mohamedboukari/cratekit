/**
 * Mock implementation of CratekitApi for the browser dev server, the GitHub
 * Pages demo and tests. In-memory state built from fixtures.ts, with simulated
 * timing: a 30 Hz playhead and meters while playing, job progress for
 * analysis / stems / model download, and MIDI learn completing after a moment.
 * Never loaded in production Tauri builds (see initApi in lib/ipc.ts).
 */
import type { CratekitApi, Listener, Unlisten } from "@/lib/ipc";
import type {
  AudioDeviceChanged,
  ControlEvent,
  JobDone,
  JobFailed,
  JobKind,
  JobProgress,
  MeterReading,
  MidiBinding,
  PlaybackState,
  MidiDevice,
  MidiLearned,
  Settings,
  TrackDetail,
  TrackId,
} from "@/lib/types";
import {
  AUDIO_DEVICES,
  DEMO_SETTINGS,
  MIDI_DEVICES,
  MASTER_PRESETS,
  MIDI_MAPPINGS,
  MODELS,
  STEM_NAMES,
  makeCrates,
  makeLibrary,
  makeInstrumentalWaveform,
  makeStemWaveforms,
} from "@/mock/fixtures";

const SETTINGS_KEY = "cratekit-mock-settings";

function resolve<T>(value: T): Promise<T> {
  return Promise.resolve(value);
}
const FRAME_MS = 33;

/** Minimal typed pub/sub used for every mock event. */
class Channel<T> {
  private listeners = new Set<Listener<T>>();

  subscribe(listener: Listener<T>): Promise<Unlisten> {
    this.listeners.add(listener);
    return Promise.resolve(() => {
      this.listeners.delete(listener);
    });
  }

  emit(payload: T): void {
    for (const l of this.listeners) l(payload);
  }
}

function loadSettings(): Settings {
  try {
    const raw = globalThis.localStorage?.getItem(SETTINGS_KEY);
    if (raw) {
      const parsed: unknown = JSON.parse(raw);
      if (typeof parsed === "object" && parsed !== null)
        return { ...DEMO_SETTINGS, ...parsed };
    }
  } catch {
    // Private windows and blocked storage: fall back to defaults.
  }
  return structuredClone(DEMO_SETTINGS);
}

function saveSettings(settings: Settings): void {
  try {
    globalThis.localStorage?.setItem(SETTINGS_KEY, JSON.stringify(settings));
  } catch {
    // Persistence is a convenience in mock mode only.
  }
}

export function createMockApi(): CratekitApi {
  const library: TrackDetail[] = makeLibrary();
  const byId = new Map<TrackId, TrackDetail>(library.map((t) => [t.track.id, t]));
  const { crates, members } = makeCrates(library);
  const models = structuredClone(MODELS);
  const mappings = structuredClone(MIDI_MAPPINGS);
  let learnCount = 0;
  const settings = loadSettings();
  let nextJob = 1;

  const ch = {
    progress: new Channel<JobProgress>(),
    done: new Channel<JobDone>(),
    failed: new Channel<JobFailed>(),
    position: new Channel<number>(),
    meter: new Channel<MeterReading>(),
    playbackState: new Channel<PlaybackState>(),
    midiDevices: new Channel<MidiDevice[]>(),
    midiLearned: new Channel<MidiLearned>(),
    control: new Channel<ControlEvent>(),
    audioDevice: new Channel<AudioDeviceChanged>(),
  };

  // Playback simulation
  let loaded: TrackDetail | null = null;
  let playing = false;
  let position = 0;
  let rate = 1;
  let ticker: ReturnType<typeof setInterval> | null = null;

  function stopTicker(): void {
    if (ticker !== null) clearInterval(ticker);
    ticker = null;
  }

  function startTicker(): void {
    stopTicker();
    ticker = setInterval(() => {
      if (!loaded) return;
      position += (FRAME_MS / 1000) * rate;
      if (position >= loaded.track.durationSec) {
        position = 0;
        playing = false;
        stopTicker();
        ch.playbackState.emit({ playing: false });
      }
      ch.position.emit(position);
      const wobble = Math.sin(position * 7) * 2 + Math.random() * 1.5;
      ch.meter.emit({ peakDb: -3 + wobble * 0.5, rmsDb: -10 + wobble });
    }, FRAME_MS);
  }

  const jobTimers = new Map<string, ReturnType<typeof setInterval>>();

  let learnTimer: ReturnType<typeof setTimeout> | null = null;
  function cancelLearn(): void {
    if (learnTimer !== null) clearTimeout(learnTimer);
    learnTimer = null;
  }

  /** Runs a fake job: emits progress, applies `onStep`, then done. */
  function runJob(
    kind: JobKind,
    label: string,
    trackIds: TrackId[],
    total: number,
    stepMs: number,
    onStep?: (i: number) => void,
  ): string {
    const jobId = `mock-${nextJob++}`;
    let done = 0;
    const timer: ReturnType<typeof setInterval> = setInterval(() => {
      onStep?.(done);
      done += 1;
      ch.progress.emit({ jobId, kind, label, trackIds, done, total });
      if (done >= total) {
        clearInterval(timer);
        jobTimers.delete(jobId);
        ch.done.emit({ jobId, kind });
      }
    }, stepMs);
    jobTimers.set(jobId, timer);
    ch.progress.emit({ jobId, kind, label, trackIds, done: 0, total });
    return jobId;
  }

  return {
    appVersion: () => resolve("0.1.0-demo"),

    libraryAddPaths: () => resolve([]),
    libraryList: (q) => {
      const crateIds = q.crateId === null ? null : new Set(members.get(q.crateId) ?? []);
      const needle = q.search.trim().toLowerCase();
      const rows = library
        .filter((t) => crateIds === null || crateIds.has(t.track.id))
        .filter(
          (t) =>
            !needle ||
            `${t.track.title} ${t.track.artist}`.toLowerCase().includes(needle),
        )
        .filter((t) => q.bpmMin === null || (t.track.bpm ?? 0) >= q.bpmMin)
        .filter((t) => q.bpmMax === null || (t.track.bpm ?? Infinity) <= q.bpmMax)
        // Stands in for Rust's mixing range: within 3 BPM of the deck.
        .filter(
          (t) =>
            q.nearBpm === null ||
            (t.track.bpm !== null && Math.abs(t.track.bpm - q.nearBpm) <= 3),
        )
        .filter((t) => !q.hasStems || t.track.hasStems)
        .filter((t) => {
          if (q.matchCamelot === null) return true;
          const code = t.track.camelot;
          return (
            code === q.matchCamelot ||
            (t.analysis?.mixesWellWith.includes(q.matchCamelot) ?? false)
          );
        })
        .map((t) => structuredClone(t.track));
      return resolve(rows);
    },
    libraryCounts: () =>
      resolve({
        all: library.length,
        recentlyAdded: 12,
        needsAnalysis: library.filter((t) => !t.track.analyzed).length,
      }),
    trackDetail: (id) => {
      const t = byId.get(id);
      return t
        ? resolve(structuredClone(t))
        : Promise.reject({ code: "not_found", message: `No track ${id}` });
    },
    cratesList: () => resolve(crates.map((c) => ({ ...c }))),
    crateCreate: (name, color) => {
      const crate = { id: crates.length + 1, name, color, trackCount: 0 };
      crates.push(crate);
      members.set(crate.id, []);
      return resolve({ ...crate });
    },
    crateAddTracks: (crateId, ids) => {
      const list = members.get(crateId) ?? [];
      for (const id of ids) if (!list.includes(id)) list.push(id);
      members.set(crateId, list);
      const crate = crates.find((c) => c.id === crateId);
      if (crate) crate.trackCount = list.length;
      return resolve(undefined);
    },
    crateRemoveTracks: (crateId, ids) => {
      const list = (members.get(crateId) ?? []).filter((id) => !ids.includes(id));
      members.set(crateId, list);
      const crate = crates.find((c) => c.id === crateId);
      if (crate) crate.trackCount = list.length;
      return resolve(undefined);
    },

    setlistTransitions: (ids) => {
      // Stands in for Rust: Camelot neighbours from the fixtures, BPM change in percent.
      const pairs = ids.slice(1).map((toId, i) => {
        const fromId = ids[i] ?? toId;
        const a = byId.get(fromId);
        const b = byId.get(toId);
        const keyA = a?.track.camelot ?? null;
        const keyB = b?.track.camelot ?? null;
        const keyCompatible =
          keyA === null || keyB === null
            ? null
            : keyA === keyB || (a?.analysis?.mixesWellWith.includes(keyB) ?? false);
        const bpmA = a?.track.bpm ?? null;
        const bpmB = b?.track.bpm ?? null;
        const bpmChangePct =
          bpmA && bpmB ? Number((((bpmB - bpmA) / bpmA) * 100).toFixed(1)) : null;
        // Stands in for Rust's rule: more than 6% is a jump.
        const bpmJump = bpmChangePct !== null && Math.abs(bpmChangePct) > 6;
        return { fromId, toId, keyCompatible, bpmChangePct, bpmJump };
      });
      return resolve(pairs);
    },
    analyzeTracks: (ids) =>
      resolve(
        runJob(
          "analysis",
          `Analyzing ${ids.length} tracks`,
          ids,
          ids.length,
          400,
          (i) => {
            const t = byId.get(ids[i] ?? -1);
            if (t) t.track.analyzed = true;
          },
        ),
      ),
    jobCancel: (id) => {
      clearInterval(jobTimers.get(id));
      jobTimers.delete(id);
      return resolve(undefined);
    },
    jobsSetPaused: () => resolve(undefined),

    playbackLoad: (id) => {
      loaded = byId.get(id) ?? null;
      position = 0;
      playing = false;
      stopTicker();
      ch.position.emit(0);
      return resolve(undefined);
    },
    playbackPlay: () => {
      if (loaded) {
        playing = true;
        startTicker();
      }
      return resolve(undefined);
    },
    playbackPause: () => {
      playing = false;
      stopTicker();
      return resolve(undefined);
    },
    playbackSeek: (sec) => {
      position = Math.max(0, Math.min(sec, loaded?.track.durationSec ?? 0));
      ch.position.emit(position);
      if (playing) startTicker();
      return resolve(undefined);
    },
    playbackSetRate: (r) => {
      rate = r;
      return resolve(undefined);
    },
    playbackSetMode: () => resolve(undefined),
    playbackSetFilter: () => resolve(undefined),
    playbackSetLevelMatch: () => resolve(undefined),

    stemsSplit: (id) => {
      // Same rule as Rust: the model must be downloaded first.
      if (!models.find((m) => m.name === "htdemucs")?.downloaded)
        return Promise.reject({
          code: "model_missing",
          message: "Stems model not downloaded",
        });
      return resolve(
        runJob("stems", "Splitting stems", [id], 20, 300, (i) => {
          const t = byId.get(id);
          if (t && i === 19) t.track.hasStems = true;
        }),
      );
    },
    stemsSetGain: () => resolve(undefined),
    stemsSetMute: () => resolve(undefined),
    stemsSetSolo: () => resolve(undefined),
    stemsBackendStatus: () => resolve({ active: "coreml", reason: null }),
    stemsInfo: (id) => {
      const t = byId.get(id);
      if (!t?.track.hasStems || !t.waveform) return resolve(null);
      return resolve({
        splitSec: Number((t.track.durationSec / 9).toFixed(1)),
        waveforms: makeStemWaveforms(t.waveform),
        instrumental: makeInstrumentalWaveform(t.waveform),
      });
    },
    stemsExport: (id) => {
      const title = byId.get(id)?.track.title ?? `track-${id}`;
      return resolve(
        STEM_NAMES.map((stem) => `${settings.outputFolder}/${title} (${stem}).wav`),
      );
    },

    masterPresets: () => resolve(structuredClone(MASTER_PRESETS)),
    masterPreview: () => resolve(undefined),
    masterExport: (id, s) => {
      const title = byId.get(id)?.track.title ?? `track-${id}`;
      return resolve({
        path: `${settings.outputFolder}/${title} (master ${s.preset}).wav`,
        lufs: s.targetLufs + 0.2,
        truePeakDb: -1.1,
      });
    },

    hotcueSet: (id, slot, sec) => {
      const t = byId.get(id);
      if (t) {
        t.cues = t.cues.filter((c) => c.slot !== slot);
        t.cues.push({ slot, positionSec: sec, color: "lime", name: null });
      }
      return resolve(undefined);
    },
    hotcueClear: (id, slot) => {
      const t = byId.get(id);
      if (t) t.cues = t.cues.filter((c) => c.slot !== slot);
      return resolve(undefined);
    },

    midiListDevices: () => resolve(structuredClone(MIDI_DEVICES)),
    midiLearnStart: (target) => {
      cancelLearn();
      learnCount += 1;
      // Every third learn reuses the Play button, to show how Rust reports a moved binding.
      const reuse = learnCount % 3 === 0 && target !== "play";
      const binding: MidiBinding = reuse
        ? { kind: "note", channel: 0, number: 11 }
        : { kind: "cc", channel: 0, number: 40 + learnCount };
      learnTimer = setTimeout(() => {
        learnTimer = null;
        const previous = mappings.find(
          (m) =>
            m.target !== target &&
            m.binding?.kind === binding.kind &&
            m.binding.channel === binding.channel &&
            m.binding.number === binding.number,
        );
        if (previous) previous.binding = null;
        const row = mappings.find((m) => m.target === target);
        if (row) row.binding = binding;
        ch.midiLearned.emit({ target, binding, replaced: previous?.target ?? null });
      }, 1200);
      return resolve(undefined);
    },
    midiLearnCancel: () => {
      cancelLearn();
      return resolve(undefined);
    },
    midiMappings: (deviceId) =>
      resolve(structuredClone(mappings.filter((m) => m.deviceId === deviceId))),

    exportVirtualdj: (ids, dryRun) =>
      resolve({
        dryRun,
        backupPath:
          "~/Library/Application Support/io.github.mohamedboukari.cratekit/backups/virtualdj/2026-11-15",
        tracks: ids.flatMap((id) => {
          const t = byId.get(id);
          if (!t) return [];
          return [
            {
              trackId: id,
              title: t.track.title,
              changes: [
                `BPM ${t.track.bpm ?? "?"}`,
                `Key ${t.track.camelot ?? "?"}`,
                `${t.cues.length} hot cues`,
              ],
            },
          ];
        }),
      }),

    modelsStatus: () => resolve(structuredClone(models)),
    modelsDownload: (name) =>
      resolve(
        runJob("model-download", `Downloading ${name}`, [], 25, 200, (i) => {
          const m = models.find((x) => x.name === name);
          if (m && i === 24) m.downloaded = true;
        }),
      ),
    modelsDelete: (name) => {
      const m = models.find((x) => x.name === name);
      if (m) m.downloaded = false;
      return resolve(undefined);
    },

    audioListDevices: () => resolve(structuredClone(AUDIO_DEVICES)),
    audioSetDevice: (id) => {
      settings.audioDeviceId = id;
      saveSettings(settings);
      ch.audioDevice.emit({ id, reason: "selected" });
      return resolve(undefined);
    },
    audioSetBufferSize: (frames) => {
      settings.bufferSize = frames;
      saveSettings(settings);
      return resolve(undefined);
    },

    settingsGet: () => resolve(structuredClone(settings)),
    settingsSet: (key, value) => {
      settings[key] = value;
      saveSettings(settings);
      return resolve(undefined);
    },

    onJobProgress: (l) => ch.progress.subscribe(l),
    onJobDone: (l) => ch.done.subscribe(l),
    onJobFailed: (l) => ch.failed.subscribe(l),
    onPlaybackPosition: (l) => ch.position.subscribe(l),
    onMeter: (l) => ch.meter.subscribe(l),
    onPlaybackState: (l) => ch.playbackState.subscribe(l),
    onMidiDeviceChanged: (l) => ch.midiDevices.subscribe(l),
    onMidiLearned: (l) => ch.midiLearned.subscribe(l),
    onControlEvent: (l) => ch.control.subscribe(l),
    onAudioDeviceChanged: (l) => ch.audioDevice.subscribe(l),
  };
}
