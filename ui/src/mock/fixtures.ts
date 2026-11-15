/**
 * Deterministic fake library for mock mode (browser dev server, Pages demo,
 * tests). Values that Rust computes in the real app (Camelot codes, matches,
 * energy, loudness) are precomputed here so the UI never computes them.
 * All titles and artists are invented.
 */
import type {
  Analysis,
  AudioDevice,
  Crate,
  Cue,
  CueColor,
  CueSlot,
  MidiDevice,
  MidiMapping,
  ModelStatus,
  Settings,
  TrackDetail,
  TrackRow,
  Waveform,
} from "@/lib/types";
import { DEFAULT_SETTINGS } from "@/lib/defaults";

/** Small seeded PRNG (mulberry32) so every run shows the same library. */
function rng(seed: number): () => number {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6d2b79f5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const NOTES = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"] as const;

/** docs/SPEC.md Camelot table: pitch class to [major, minor]. */
const CAMELOT: ReadonlyArray<readonly [string, string]> = [
  ["8B", "5A"],
  ["3B", "12A"],
  ["10B", "7A"],
  ["5B", "2A"],
  ["12B", "9A"],
  ["7B", "4A"],
  ["2B", "11A"],
  ["9B", "6A"],
  ["4B", "1A"],
  ["11B", "8A"],
  ["6B", "3A"],
  ["1B", "10A"],
];

/** Keeps a Camelot number on the 1 to 12 wheel. */
function wrap(n: number): number {
  return ((n + 11) % 12) + 1;
}

/** Same number other letter, and +/-1 on the wheel with the same letter. */
function mixesWellWith(camelot: string): string[] {
  const num = Number.parseInt(camelot, 10);
  const letter = camelot.slice(-1);
  const other = letter === "A" ? "B" : "A";
  return [`${num}${other}`, `${wrap(num - 1)}${letter}`, `${wrap(num + 1)}${letter}`];
}

const FIRST = [
  "Midnight",
  "Glass",
  "Low",
  "Golden",
  "Neon",
  "Desert",
  "Velvet",
  "Electric",
  "Silent",
  "Crimson",
  "Ocean",
  "Paper",
];
const SECOND = [
  "Drive",
  "City",
  "Tide",
  "Hour",
  "Pulse",
  "Lights",
  "Rooms",
  "Signal",
  "Motion",
  "Garden",
  "Echo",
  "Shift",
];
const VERSIONS = ["", " (Extended Mix)", " (Dub)", " (Club Edit)", " (Radio Edit)"];
const ARTISTS = [
  "Demo Artist",
  "Kora Lane",
  "Sable",
  "Northline",
  "Medina Sound",
  "Ilyes",
  "Vera Kai",
  "Atlas Grey",
  "Noor",
  "Halcyon Club",
];
const CUE_COLORS: CueColor[] = [
  "orange",
  "blue",
  "pink",
  "purple",
  "lime",
  "yellow",
  "red",
  "teal",
];
const CUE_SLOTS: readonly CueSlot[] = [1, 2, 3, 4, 5, 6, 7, 8];

export const TRACK_COUNT = 60;

function makeWaveform(rand: () => number, columns: number): Waveform {
  const peaks: number[] = [];
  const rms: number[] = [];
  for (let i = 0; i < columns; i++) {
    const x = i / columns;
    // Intro, breakdown and outro are quieter, like a typical club track.
    const env = x < 0.08 ? 0.35 : x > 0.88 ? 0.4 : x > 0.42 && x < 0.5 ? 0.3 : 1;
    const peak = Math.min(
      1,
      env * (0.45 + 0.55 * Math.abs(Math.sin(i * 0.37) * Math.cos(i * 0.051))) +
        rand() * 0.08,
    );
    peaks.push(Number(peak.toFixed(3)));
    rms.push(Number((peak * (0.55 + rand() * 0.1)).toFixed(3)));
  }
  return { peaks, rms };
}

function makeTrack(id: number): TrackDetail {
  const rand = rng(id * 7919);
  const pitch = Math.floor(rand() * 12);
  const minor = rand() < 0.65;
  const camelotPair = CAMELOT[pitch] ?? ["8B", "5A"];
  const camelot = minor ? camelotPair[1] : camelotPair[0];
  const keyName = `${NOTES[pitch] ?? "C"} ${minor ? "minor" : "major"}`;
  const durationSec = Math.round(300 + rand() * 180);
  const bpm = Number((118 + rand() * 12).toFixed(1));
  const energy = 3 + Math.floor(rand() * 7);
  const analyzed = id % 9 !== 0;

  const track: TrackRow = {
    id,
    title: `${FIRST[id % FIRST.length]} ${SECOND[(id * 5) % SECOND.length]}${VERSIONS[id % VERSIONS.length]}`,
    artist: ARTISTS[(id * 3) % ARTISTS.length] ?? "Demo Artist",
    durationSec,
    bpm: analyzed ? bpm : null,
    camelot: analyzed ? camelot : null,
    keyName: analyzed ? keyName : null,
    energy: analyzed ? energy : null,
    lufs: analyzed ? Number((-6 - rand() * 8).toFixed(1)) : null,
    analyzed,
    hasStems: analyzed && id % 3 === 0,
    mastered: analyzed && id % 7 === 0,
    activity: null,
  };

  const scale = minor ? [0, 3, 7] : [0, 4, 7];
  const chroma = NOTES.map((_, i) => {
    const inChord = scale.includes((i - pitch + 12) % 12);
    return Number((inChord ? 0.7 + rand() * 0.3 : rand() * 0.35).toFixed(2));
  });

  const analysis: Analysis | null = analyzed
    ? {
        bpm,
        bpmConfidence: Number((0.8 + rand() * 0.2).toFixed(2)),
        keyName,
        camelot,
        keyConfidence: Number((0.7 + rand() * 0.3).toFixed(2)),
        energy,
        lufs: track.lufs ?? -9,
        truePeakDb: Number((-0.2 - rand() * 1.5).toFixed(1)),
        firstBeatSec: Number((rand() * 0.5).toFixed(3)),
        chroma,
        mixesWellWith: mixesWellWith(camelot),
        dropSec: Math.round(durationSec * 0.38),
        mixOutSec: Math.round(durationSec * 0.84),
      }
    : null;

  const cueCount = analyzed ? 4 + (id % 5) : 0;
  const cues: Cue[] = [];
  for (let s = 1; s <= cueCount; s++) {
    const slot = CUE_SLOTS[s - 1];
    if (slot === undefined) break;
    cues.push({
      slot,
      positionSec: Math.round((durationSec * s) / (cueCount + 1)),
      color: CUE_COLORS[s - 1] ?? "lime",
      name: s === 1 ? "Intro" : s === 3 ? "Drop" : null,
    });
  }

  return { track, analysis, cues, waveform: analyzed ? makeWaveform(rand, 600) : null };
}

export function makeLibrary(count: number = TRACK_COUNT): TrackDetail[] {
  return Array.from({ length: count }, (_, i) => makeTrack(i + 1));
}

export function makeCrates(tracks: TrackDetail[]): {
  crates: Crate[];
  members: Map<number, number[]>;
} {
  const defs: Array<{ name: string; color: CueColor; pick: (id: number) => boolean }> = [
    { name: "Friday club set", color: "lime", pick: (id) => id % 2 === 0 },
    { name: "Wedding, 25 Nov", color: "pink", pick: (id) => id % 3 === 1 },
    { name: "Afro house", color: "blue", pick: (id) => id % 4 === 0 },
    { name: "Edits and acapellas", color: "purple", pick: (id) => id % 5 === 0 },
  ];
  const members = new Map<number, number[]>();
  const crates = defs.map((d, i) => {
    const ids = tracks.map((t) => t.track.id).filter(d.pick);
    members.set(i + 1, ids);
    return { id: i + 1, name: d.name, color: d.color, trackCount: ids.length };
  });
  return { crates, members };
}

export const AUDIO_DEVICES: AudioDevice[] = [
  { id: "builtin", name: "MacBook Speakers", isDefault: true, sampleRate: 48000 },
  { id: "ddj-flx4", name: "DDJ-FLX4 Audio", isDefault: false, sampleRate: 44100 },
];

export const MIDI_DEVICES: MidiDevice[] = [
  { id: "ddj-flx4", name: "Pioneer DDJ-FLX4", connected: true },
];

export const MIDI_MAPPINGS: MidiMapping[] = [
  {
    deviceId: "ddj-flx4",
    target: "play",
    binding: { kind: "note", channel: 0, number: 11 },
  },
  {
    deviceId: "ddj-flx4",
    target: "cue",
    binding: { kind: "note", channel: 0, number: 12 },
  },
  {
    deviceId: "ddj-flx4",
    target: "hotcue-1",
    binding: { kind: "note", channel: 7, number: 0 },
  },
  {
    deviceId: "ddj-flx4",
    target: "hotcue-2",
    binding: { kind: "note", channel: 7, number: 1 },
  },
  {
    deviceId: "ddj-flx4",
    target: "hotcue-3",
    binding: { kind: "note", channel: 7, number: 2 },
  },
  {
    deviceId: "ddj-flx4",
    target: "hotcue-4",
    binding: { kind: "note", channel: 7, number: 3 },
  },
  {
    deviceId: "ddj-flx4",
    target: "mute-vocals",
    binding: { kind: "note", channel: 7, number: 4 },
  },
  { deviceId: "ddj-flx4", target: "tempo", binding: null },
];

export const MODELS: ModelStatus[] = [
  { name: "htdemucs", sizeBytes: 209_715_200, downloaded: false },
];

/** Demo settings: the real defaults plus a device and folders, so screens look filled. */
export const DEMO_SETTINGS: Settings = {
  ...DEFAULT_SETTINGS,
  audioDeviceId: "builtin",
  watchedFolders: ["~/Music/DJ"],
  watchFolders: true,
  outputFolder: "~/Music/Cratekit",
};
