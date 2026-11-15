import { cn } from "cn";
import {
  useEffect,
  useRef,
  useState,
  type JSX,
  type KeyboardEvent,
  type PointerEvent,
} from "react";

import {
  drawWaveform,
  readWaveformColors,
  xToTime,
  type WaveformColors,
} from "@/canvas/waveform";
import { CueEditDialog } from "@/components/deck/CueEditDialog";
import { CUE_BG } from "@/components/deck/cueColors";
import { usePlaybackPositionRef } from "@/hooks/usePlaybackRefs";
import { formatTime } from "@/lib/format";
import type { Cue, TrackDetail } from "@/lib/types";
import { usePlayer } from "@/stores/player";

const SEEK_STEP_SEC = 5;
const SEEK_BIG_STEP_SEC = 15;
/** While playing, the slider's spoken value refreshes this often (not every second). */
const PLAYING_ANNOUNCE_SEC = 10;
/** Pointer travel before a press on a cue flag counts as a drag, in CSS px. */
const DRAG_THRESHOLD_PX = 3;

interface WaveformProps {
  detail: TrackDetail;
}

/**
 * Deck waveform. The canvas is redrawn in a requestAnimationFrame loop from
 * the playhead ref (no React re-render at 30 Hz). The wrapper is an accessible
 * slider for the track position; its aria values are updated directly on the
 * DOM (each second while paused, every few seconds while playing). Hot cue
 * flags sit in a sibling group above it: real buttons, click to jump, drag to
 * move, Delete to clear.
 */
export function Waveform({ detail }: WaveformProps): JSX.Element {
  const position = usePlaybackPositionRef();
  const seek = usePlayer((s) => s.seek);
  const setHotCue = usePlayer((s) => s.setHotCue);
  const clearHotCue = usePlayer((s) => s.clearHotCue);
  // The cue being edited, tied to its track so a track change never edits the wrong cue.
  const [editing, setEditing] = useState<{ trackId: number; cue: Cue } | null>(null);
  const sliderRef = useRef<HTMLDivElement>(null);
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const { track, waveform, cues, analysis } = detail;
  const duration = track.durationSec;

  // Draw loop: redraws only when the position, size or data changed.
  useEffect(() => {
    const canvas = canvasRef.current;
    const slider = sliderRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !slider || !ctx || !waveform) return;
    let colors: WaveformColors = readWaveformColors(document.documentElement);
    let frame = 0;
    let lastDrawn = -1;
    let lastAnnounced = -1;
    let dirty = true;

    const resize = (): void => {
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr));
      canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr));
      colors = readWaveformColors(document.documentElement);
      dirty = true;
    };
    const observer = new ResizeObserver(resize);
    observer.observe(canvas);
    resize();

    const loop = (): void => {
      const pos = position.current;
      if (dirty || pos !== lastDrawn) {
        drawWaveform(
          ctx,
          {
            width: canvas.width,
            height: canvas.height,
            dpr: window.devicePixelRatio || 1,
            waveform,
            durationSec: duration,
            positionSec: pos,
            cues,
            bpm: analysis?.bpm ?? null,
            firstBeatSec: analysis?.firstBeatSec ?? 0,
          },
          colors,
        );
        lastDrawn = pos;
        dirty = false;
      }
      const second = Math.floor(pos);
      const due = usePlayer.getState().playing
        ? Math.abs(second - lastAnnounced) >= PLAYING_ANNOUNCE_SEC
        : second !== lastAnnounced;
      if (due) {
        slider.setAttribute("aria-valuenow", String(second));
        slider.setAttribute(
          "aria-valuetext",
          `${formatTime(pos)} of ${formatTime(duration)}`,
        );
        lastAnnounced = second;
      }
      frame = requestAnimationFrame(loop);
    };
    frame = requestAnimationFrame(loop);
    return () => {
      cancelAnimationFrame(frame);
      observer.disconnect();
    };
  }, [position, waveform, cues, duration, analysis]);

  const seekFromPointer = (event: PointerEvent<HTMLCanvasElement>): void => {
    const rect = event.currentTarget.getBoundingClientRect();
    void seek(xToTime(event.clientX - rect.left, rect.width, duration));
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const step = event.shiftKey ? SEEK_BIG_STEP_SEC : SEEK_STEP_SEC;
    let target: number | null = null;
    if (event.key === "ArrowRight") target = position.current + step;
    else if (event.key === "ArrowLeft") target = position.current - step;
    else if (event.key === "Home") target = 0;
    else if (event.key === "End") target = Math.max(0, duration - 1);
    if (target === null) return;
    event.preventDefault();
    void seek(Math.max(0, Math.min(duration, target)));
  };

  if (!waveform) {
    return (
      <div className="grid h-full place-items-center rounded-md border border-dashed border-border text-muted-foreground">
        Not analyzed yet: the waveform appears after analysis
      </div>
    );
  }

  return (
    <div className="relative h-full min-h-16">
      <div
        ref={sliderRef}
        // Custom widget (rotary knob / waveform scrubber): no native input can render it;
        // it implements the WAI-ARIA slider pattern (keys, aria-value*) itself.
        // oxlint-disable-next-line jsx-a11y/prefer-tag-over-role
        role="slider"
        tabIndex={0}
        aria-label="Track position"
        aria-valuemin={0}
        aria-valuemax={Math.floor(duration)}
        aria-valuenow={0}
        onKeyDown={onKeyDown}
        className="absolute inset-0 rounded-md"
      >
        <canvas
          ref={canvasRef}
          aria-hidden="true"
          onPointerDown={seekFromPointer}
          className="block size-full cursor-default"
        />
      </div>
      {/* Slider children are presentational in ARIA, so the flags live beside it. */}
      <fieldset className="pointer-events-none absolute inset-0 min-w-0">
        <legend className="sr-only">Hot cues</legend>
        {cues.map((cue) => (
          <CueFlag
            key={cue.slot}
            cue={cue}
            duration={duration}
            onJump={() => void seek(cue.positionSec)}
            onMove={(sec) => void setHotCue(cue.slot, sec)}
            onClear={() => void clearHotCue(cue.slot)}
            onEdit={() => setEditing({ trackId: track.id, cue })}
          />
        ))}
      </fieldset>
      <CueEditDialog
        cue={editing?.trackId === track.id ? editing.cue : null}
        returnFocus={sliderRef}
        onClose={() => setEditing(null)}
      />
    </div>
  );
}

/** The cue lane (the flag's parent) on screen. */
function lane(event: PointerEvent<HTMLButtonElement>): DOMRect | undefined {
  return event.currentTarget.parentElement?.getBoundingClientRect();
}

interface CueFlagProps {
  cue: Cue;
  duration: number;
  onJump: () => void;
  onMove: (sec: number) => void;
  onClear: () => void;
  /** Right-click (or the context menu key): rename, recolour, delete. */
  onEdit: () => void;
}

function CueFlag({
  cue,
  duration,
  onJump,
  onMove,
  onClear,
  onEdit,
}: CueFlagProps): JSX.Element {
  const [dragSec, setDragSec] = useState<number | null>(null);
  /** Pointer x at press, and how far it was from the cue line (so the flag does not jump). */
  const press = useRef<{ x: number; offset: number } | null>(null);
  const moved = useRef(false);
  const shown = dragSec ?? cue.positionSec;
  const pct = duration > 0 ? (shown / duration) * 100 : 0;

  const reset = (): void => {
    press.current = null;
    setDragSec(null);
  };

  return (
    <button
      type="button"
      aria-label={`Hot cue ${cue.slot} at ${formatTime(cue.positionSec)}${cue.name ? `, ${cue.name}` : ""}. Delete to clear, Shift+Enter to edit`}
      aria-keyshortcuts="Delete Shift+Enter F2"
      onContextMenu={(e) => {
        e.preventDefault();
        onEdit();
      }}
      onPointerDown={(e) => {
        e.stopPropagation();
        // Only the primary button drags; a right-click opens the editor without moving the cue.
        if (e.button !== 0) return;
        const rect = lane(e);
        const lineX =
          rect && duration > 0
            ? rect.left + (cue.positionSec / duration) * rect.width
            : e.clientX;
        press.current = { x: e.clientX, offset: e.clientX - lineX };
        moved.current = false;
        e.currentTarget.setPointerCapture(e.pointerId);
      }}
      onPointerMove={(e) => {
        const start = press.current;
        if (!start || !e.currentTarget.hasPointerCapture(e.pointerId)) return;
        if (!moved.current && Math.abs(e.clientX - start.x) < DRAG_THRESHOLD_PX) return;
        moved.current = true;
        const rect = lane(e);
        if (rect)
          setDragSec(xToTime(e.clientX - start.offset - rect.left, rect.width, duration));
      }}
      onPointerUp={(e) => {
        if (!e.currentTarget.hasPointerCapture(e.pointerId)) return;
        e.currentTarget.releasePointerCapture(e.pointerId);
        if (moved.current && dragSec !== null) onMove(dragSec);
        reset();
      }}
      onPointerCancel={reset}
      onLostPointerCapture={() => {
        if (press.current) reset();
      }}
      onClick={() => {
        if (!moved.current) onJump();
      }}
      onKeyDown={(e) => {
        // Mac keyboards have no context menu key, so Shift+Enter and F2 open the editor too.
        if (
          (e.key === "Enter" && e.shiftKey) ||
          e.key === "F2" ||
          e.key === "ContextMenu"
        ) {
          e.preventDefault();
          onEdit();
          return;
        }
        if (e.key === "Delete" || e.key === "Backspace") {
          e.preventDefault();
          onClear();
        }
      }}
      style={{ left: `${pct}%` }}
      // A 12 px wide hit area around the 2 px line, so the flag is easy to grab.
      className="pointer-events-auto absolute top-0 bottom-3 w-3 -translate-x-1/2 cursor-ew-resize"
    >
      <span
        aria-hidden="true"
        className={cn(
          "absolute inset-y-0 left-1/2 w-0.5 -translate-x-px",
          CUE_BG[cue.color],
        )}
      />
      <span
        aria-hidden="true"
        className={cn(
          "absolute -top-0.5 left-1/2 rounded-sm px-1 font-mono text-[11px] leading-4 font-bold text-ck-ink",
          CUE_BG[cue.color],
        )}
      >
        {cue.slot}
      </span>
    </button>
  );
}
