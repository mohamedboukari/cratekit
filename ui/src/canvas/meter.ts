/**
 * Output level meters for the Master tab: imperative drawing only, so it can
 * run in a requestAnimationFrame loop from the meter ref (no React state).
 */
import type { Paint } from "@/canvas/waveform";
import type { MeterReading } from "@/lib/types";

/** The quietest level shown; anything below draws as empty. */
export const METER_FLOOR_DB = -60;

export interface MeterColors {
  track: string;
  fill: string;
  /** The part of the peak bar above the ceiling. */
  hot: string;
  mark: string;
}

/** Reads the meter colours from the design tokens once (not every frame). */
export function readMeterColors(root: HTMLElement): MeterColors {
  const css = getComputedStyle(root);
  const v = (name: string, fallback: string): string =>
    css.getPropertyValue(name).trim() || fallback;
  return {
    track: v("--color-ck-panel2", "#1b1f25"),
    fill: v("--color-ck-lime", "#c6f432"),
    hot: v("--color-ck-orange", "#ff7a45"),
    mark: v("--color-ck-text", "#e9ebee"),
  };
}

/** Where a level sits on the meter, 0 (floor) to 1 (0 dBFS). */
export function dbToFraction(db: number): number {
  if (!Number.isFinite(db)) return 0;
  return Math.max(0, Math.min(1, (db - METER_FLOOR_DB) / -METER_FLOOR_DB));
}

/**
 * Two horizontal bars on one canvas: peak on top (with the ceiling mark and
 * anything above it in the hot colour), RMS below. Sizes are device pixels.
 */
export function drawMeter(
  ctx: Paint,
  width: number,
  height: number,
  reading: MeterReading,
  colors: MeterColors,
  ceilingDb: number,
): void {
  const gap = Math.max(2, Math.round(height * 0.2));
  const barHeight = Math.max(1, Math.floor((height - gap) / 2));
  const ceilingX = Math.round(dbToFraction(ceilingDb) * width);

  const bar = (y: number, db: number, withCeiling: boolean): void => {
    ctx.fillStyle = colors.track;
    ctx.fillRect(0, y, width, barHeight);
    const fillX = Math.round(dbToFraction(db) * width);
    ctx.fillStyle = colors.fill;
    ctx.fillRect(0, y, withCeiling ? Math.min(fillX, ceilingX) : fillX, barHeight);
    if (withCeiling && fillX > ceilingX) {
      ctx.fillStyle = colors.hot;
      ctx.fillRect(ceilingX, y, fillX - ceilingX, barHeight);
    }
    if (withCeiling) {
      ctx.fillStyle = colors.mark;
      ctx.fillRect(Math.min(width - 1, ceilingX), y, 1, barHeight);
    }
  };

  bar(0, reading.peakDb, true);
  bar(barHeight + gap, reading.rmsDb, false);
}
