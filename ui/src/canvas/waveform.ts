/**
 * Pure drawing code for the deck waveform (no React, testable with a fake
 * context). Called from a requestAnimationFrame loop with the latest playhead
 * position, so it must stay cheap and allocation-free per frame.
 */
import type { Cue, CueColor, Waveform } from "@/lib/types";

/** The only canvas operations the waveform uses (keeps it testable without a real canvas). */
export type Paint = Pick<CanvasRenderingContext2D, "fillRect" | "fillStyle">;

export interface WaveformColors {
  background: string;
  played: string;
  unplayed: string;
  playedRms: string;
  unplayedRms: string;
  grid: string;
  playhead: string;
  overviewTrack: string;
  overviewFill: string;
  cue: Record<CueColor, string>;
}

export interface WaveformFrame {
  /** Canvas size in device pixels. */
  width: number;
  height: number;
  /** devicePixelRatio, used for line widths. */
  dpr: number;
  waveform: Waveform;
  durationSec: number;
  positionSec: number;
  cues: readonly Cue[];
  bpm: number | null;
  firstBeatSec: number;
}

/** Height of the overview strip at the bottom, in CSS pixels. */
export const OVERVIEW_CSS_PX = 8;
/** Gap between the waveform and the overview strip, in CSS pixels. */
const GAP_CSS_PX = 4;
/** Beats per phrase line drawn on the grid (16 beats = 4 bars). */
const PHRASE_BEATS = 16;

/** Reads the colours once from the design tokens in ui/src/index.css. */
export function readWaveformColors(root: HTMLElement): WaveformColors {
  const css = getComputedStyle(root);
  const v = (name: string, fallback: string): string =>
    css.getPropertyValue(name).trim() || fallback;
  return {
    background: v("--color-ck-win", "#0f1114"),
    played: v("--color-ck-lime", "#c6f432"),
    unplayed: v("--color-ck-muted", "#858c96"),
    playedRms: "rgba(12, 14, 17, 0.35)",
    unplayedRms: "rgba(12, 14, 17, 0.45)",
    grid: "rgba(233, 235, 238, 0.07)",
    playhead: v("--color-ck-text", "#e9ebee"),
    overviewTrack: v("--color-ck-panel", "#15181d"),
    overviewFill: "rgba(198, 244, 50, 0.25)",
    cue: {
      orange: v("--color-ck-orange", "#ff7a45"),
      blue: v("--color-ck-blue", "#4cc9f0"),
      pink: v("--color-ck-pink", "#f72585"),
      purple: v("--color-ck-purple", "#9b5de5"),
      lime: v("--color-ck-lime", "#c6f432"),
      yellow: v("--color-ck-yellow", "#ffd60a"),
      red: v("--color-ck-red", "#ff4d6d"),
      teal: v("--color-ck-teal", "#2ec4b6"),
    },
  };
}

/** The x coordinate (device pixels) of a time on a canvas `width` wide. */
export function timeToX(sec: number, durationSec: number, width: number): number {
  if (durationSec <= 0) return 0;
  return Math.max(0, Math.min(width, (sec / durationSec) * width));
}

/** The time for an x coordinate (any unit, as long as `width` uses the same). */
export function xToTime(x: number, width: number, durationSec: number): number {
  if (width <= 0) return 0;
  return Math.max(0, Math.min(durationSec, (x / width) * durationSec));
}

export function drawWaveform(ctx: Paint, f: WaveformFrame, colors: WaveformColors): void {
  const { width, height, dpr, waveform, durationSec, positionSec } = f;
  const overviewH = OVERVIEW_CSS_PX * dpr;
  const mainH = Math.max(0, height - overviewH - GAP_CSS_PX * dpr);
  const mid = mainH / 2;
  const columns = waveform.peaks.length;
  const playheadX = timeToX(positionSec, durationSec, width);

  ctx.fillStyle = colors.background;
  ctx.fillRect(0, 0, width, height);

  // Phrase grid (every 16 beats from the first downbeat).
  if (f.bpm && f.bpm > 0 && durationSec > 0) {
    const phraseSec = (60 / f.bpm) * PHRASE_BEATS;
    ctx.fillStyle = colors.grid;
    for (let t = f.firstBeatSec; t < durationSec; t += phraseSec) {
      ctx.fillRect(
        Math.round(timeToX(t, durationSec, width)),
        0,
        Math.max(1, dpr),
        mainH,
      );
    }
  }

  // Waveform columns: peak in the base colour, RMS body darker on top.
  if (columns > 0) {
    const colW = width / columns;
    const barW = Math.max(1, colW * 0.7);
    for (let i = 0; i < columns; i++) {
      const x = i * colW;
      const played = x + barW <= playheadX;
      const peak = (waveform.peaks[i] ?? 0) * mid;
      const rms = (waveform.rms[i] ?? 0) * mid;
      ctx.fillStyle = played ? colors.played : colors.unplayed;
      ctx.fillRect(x, mid - peak, barW, peak * 2);
      ctx.fillStyle = played ? colors.playedRms : colors.unplayedRms;
      ctx.fillRect(x, mid - rms, barW, rms * 2);
    }
  }

  // Playhead.
  ctx.fillStyle = colors.playhead;
  ctx.fillRect(Math.round(playheadX - dpr), 0, Math.max(2, 2 * dpr), mainH);

  // Overview strip: progress plus cue ticks.
  const oy = height - overviewH;
  ctx.fillStyle = colors.overviewTrack;
  ctx.fillRect(0, oy, width, overviewH);
  ctx.fillStyle = colors.overviewFill;
  ctx.fillRect(0, oy, playheadX, overviewH);
  for (const cue of f.cues) {
    ctx.fillStyle = colors.cue[cue.color];
    ctx.fillRect(
      Math.round(timeToX(cue.positionSec, durationSec, width)),
      oy,
      Math.max(2, 2 * dpr),
      overviewH,
    );
  }
}
