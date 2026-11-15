import { describe, expect, it } from "vitest";

import {
  drawWaveform,
  timeToX,
  type Paint,
  xToTime,
  type WaveformColors,
  type WaveformFrame,
} from "@/canvas/waveform";

/** Records fillRect calls with the fillStyle in effect, like a tiny canvas spy. */
function fakeContext(): {
  ctx: Paint;
  rects: Array<{ style: string; x: number; y: number; w: number; h: number }>;
} {
  const rects: Array<{ style: string; x: number; y: number; w: number; h: number }> = [];
  const ctx: Paint = {
    fillStyle: "",
    fillRect(x: number, y: number, w: number, h: number) {
      rects.push({
        style: typeof ctx.fillStyle === "string" ? ctx.fillStyle : "non-string",
        x,
        y,
        w,
        h,
      });
    },
  };
  return { ctx, rects };
}

const colors: WaveformColors = {
  background: "bg",
  played: "played",
  unplayed: "unplayed",
  playedRms: "playedRms",
  unplayedRms: "unplayedRms",
  grid: "grid",
  playhead: "playhead",
  overviewTrack: "ovTrack",
  overviewFill: "ovFill",
  cue: {
    orange: "c-orange",
    blue: "c-blue",
    pink: "c-pink",
    purple: "c-purple",
    lime: "c-lime",
    yellow: "c-yellow",
    red: "c-red",
    teal: "c-teal",
  },
};

function frame(overrides: Partial<WaveformFrame> = {}): WaveformFrame {
  return {
    width: 1000,
    height: 100,
    dpr: 1,
    waveform: {
      peaks: Array.from({ length: 100 }, () => 0.5),
      rms: Array.from({ length: 100 }, () => 0.25),
    },
    durationSec: 200,
    positionSec: 50,
    cues: [{ slot: 1, positionSec: 100, color: "orange", name: null }],
    bpm: 120,
    firstBeatSec: 0,
    ...overrides,
  };
}

describe("waveform drawing", () => {
  it("maps time and x both ways and clamps to the track", () => {
    expect(timeToX(50, 200, 1000)).toBe(250);
    expect(timeToX(-5, 200, 1000)).toBe(0);
    expect(timeToX(500, 200, 1000)).toBe(1000);
    expect(xToTime(250, 1000, 200)).toBe(50);
    expect(xToTime(5000, 1000, 200)).toBe(200);
  });

  it("colours columns before the playhead as played and after it as unplayed", () => {
    const { ctx, rects } = fakeContext();
    drawWaveform(ctx, frame(), colors);
    const played = rects.filter((r) => r.style === "played");
    const unplayed = rects.filter((r) => r.style === "unplayed");
    // Playhead at 25%: about a quarter of 100 columns are played.
    expect(played.length).toBeGreaterThanOrEqual(23);
    expect(played.length).toBeLessThanOrEqual(25);
    expect(played.length + unplayed.length).toBe(100);
    expect(Math.max(...played.map((r) => r.x))).toBeLessThan(250);
  });

  it("draws the playhead, the phrase grid and each cue on the overview", () => {
    const { ctx, rects } = fakeContext();
    drawWaveform(ctx, frame(), colors);
    const playhead = rects.find((r) => r.style === "playhead");
    expect(playhead?.x).toBeCloseTo(249, 0);
    // 120 BPM, 16 beats per phrase = 8 s; 200 s track = 25 phrase lines.
    expect(rects.filter((r) => r.style === "grid")).toHaveLength(25);
    const cue = rects.find((r) => r.style === "c-orange");
    expect(cue?.x).toBe(500);
  });

  it("skips the grid when the track has no BPM yet", () => {
    const { ctx, rects } = fakeContext();
    drawWaveform(ctx, frame({ bpm: null }), colors);
    expect(rects.some((r) => r.style === "grid")).toBe(false);
  });
});
