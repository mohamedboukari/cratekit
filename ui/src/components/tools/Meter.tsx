import { useEffect, useRef, type JSX } from "react";
import { useTranslation } from "react-i18next";

import { drawMeter, readMeterColors, type MeterColors } from "@/canvas/meter";
import { useMeterRef } from "@/hooks/usePlaybackRefs";
import type { MeterReading } from "@/lib/types";
import { usePlayer } from "@/stores/player";

/** Readouts refresh this often; the bars redraw every frame the level changes. */
const READOUT_MS = 150;
const SILENCE: MeterReading = { peakDb: -96, rmsDb: -96 };

function dbText(db: number): string {
  return db <= -60 ? "-inf" : db.toFixed(1);
}

/**
 * Output meters (peak with the -1 dBTP ceiling, and RMS). Drawn on a canvas in
 * a requestAnimationFrame loop from the meter ref; the number readouts are
 * written to the DOM directly, a few times a second. React never re-renders
 * for level changes.
 */
export function Meter({ ceilingDb }: { ceilingDb: number }): JSX.Element {
  const { t } = useTranslation();
  const meter = useMeterRef();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const peakRef = useRef<HTMLSpanElement>(null);
  const rmsRef = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    let colors: MeterColors = readMeterColors(document.documentElement);
    let frame = 0;
    let last: MeterReading | null = null;
    let lastReadout = 0;
    let dirty = true;

    const resize = (): void => {
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr));
      canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr));
      colors = readMeterColors(document.documentElement);
      dirty = true;
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();

    const loop = (now: number): void => {
      // Paused: the meters fall to silence instead of freezing on the last level.
      const reading = usePlayer.getState().playing ? meter.current : SILENCE;
      if (dirty || reading !== last) {
        drawMeter(ctx, canvas.width, canvas.height, reading, colors, ceilingDb);
        last = reading;
        dirty = false;
      }
      if (now - lastReadout > READOUT_MS) {
        if (peakRef.current) peakRef.current.textContent = dbText(reading.peakDb);
        if (rmsRef.current) rmsRef.current.textContent = dbText(reading.rmsDb);
        lastReadout = now;
      }
      frame = requestAnimationFrame(loop);
    };
    frame = requestAnimationFrame(loop);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [meter, ceilingDb]);

  return (
    <div className="grid grid-cols-[44px_1fr_52px] items-center gap-x-2 gap-y-1">
      <span className="text-[11px] text-muted-foreground">{t("tools.master.peak")}</span>
      <canvas ref={canvasRef} aria-hidden="true" className="row-span-2 h-7 w-full" />
      <span className="text-right font-mono text-[11px] tabular-nums">
        <span ref={peakRef}>-inf</span> dB
      </span>
      <span className="text-[11px] text-muted-foreground">RMS</span>
      <span className="text-right font-mono text-[11px] tabular-nums">
        <span ref={rmsRef}>-inf</span> dB
      </span>
    </div>
  );
}
