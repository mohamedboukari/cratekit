import { describe, expect, it } from "vitest";

import { dbToFraction, drawMeter, type MeterColors } from "@/canvas/meter";
import type { Paint } from "@/canvas/waveform";

const colors: MeterColors = { track: "track", fill: "fill", hot: "hot", mark: "mark" };

function fakeContext(): {
  ctx: Paint;
  rects: Array<{ style: string; x: number; w: number }>;
} {
  const rects: Array<{ style: string; x: number; w: number }> = [];
  const ctx: Paint = {
    fillStyle: "",
    fillRect(x: number, _y: number, w: number) {
      rects.push({
        style: typeof ctx.fillStyle === "string" ? ctx.fillStyle : "?",
        x,
        w,
      });
    },
  };
  return { ctx, rects };
}

describe("meter drawing", () => {
  it("maps decibels onto the meter and clamps", () => {
    expect(dbToFraction(0)).toBe(1);
    expect(dbToFraction(-30)).toBe(0.5);
    expect(dbToFraction(-96)).toBe(0);
    expect(dbToFraction(3)).toBe(1);
    expect(dbToFraction(Number.NEGATIVE_INFINITY)).toBe(0);
  });

  it("draws the part of the peak above the ceiling in the hot colour", () => {
    const { ctx, rects } = fakeContext();
    drawMeter(ctx, 600, 20, { peakDb: -0.5, rmsDb: -12 }, colors, -1);
    const hot = rects.filter((r) => r.style === "hot");
    expect(hot).toHaveLength(1);
    expect(hot[0]?.x).toBe(590);
    expect(rects.some((r) => r.style === "mark")).toBe(true);
  });

  it("has no hot part while the peak stays under the ceiling", () => {
    const { ctx, rects } = fakeContext();
    drawMeter(ctx, 600, 20, { peakDb: -6, rmsDb: -14 }, colors, -1);
    expect(rects.some((r) => r.style === "hot")).toBe(false);
  });
});
