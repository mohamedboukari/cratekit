import { Pause, Play } from "lucide-react";
import { useEffect, useRef, type JSX } from "react";

import { Button } from "@/components/ui/button";
import { usePlaybackPositionRef, useMeterRef } from "@/hooks/usePlaybackRefs";
import { usePlayer } from "@/stores/player";

function formatTime(sec: number): string {
  const s = Math.max(0, Math.floor(sec));
  return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
}

/**
 * Deck (foundation version): loaded track, play/pause, live time and level.
 * PR B replaces the body with the waveform, cues and transport.
 *
 * The time and level update at about 30 Hz without React re-renders: a
 * requestAnimationFrame loop reads the refs and writes to the DOM directly.
 */
export function Deck(): JSX.Element {
  const detail = usePlayer((s) => s.detail);
  const playing = usePlayer((s) => s.playing);
  const togglePlay = usePlayer((s) => s.togglePlay);
  const position = usePlaybackPositionRef();
  const meter = useMeterRef();
  const timeEl = useRef<HTMLSpanElement>(null);
  const levelEl = useRef<HTMLSpanElement>(null);

  useEffect(() => {
    let frame = 0;
    const draw = (): void => {
      if (timeEl.current) timeEl.current.textContent = formatTime(position.current);
      if (levelEl.current) {
        // -48 dBFS (empty) to 0 dBFS (full).
        const pct = Math.max(0, Math.min(100, ((meter.current.rmsDb + 48) / 48) * 100));
        levelEl.current.style.width = `${pct}%`;
      }
      frame = requestAnimationFrame(draw);
    };
    frame = requestAnimationFrame(draw);
    return () => cancelAnimationFrame(frame);
  }, [position, meter]);

  if (!detail) {
    return (
      <section
        aria-label="Deck"
        className="grid h-full place-items-center border-b border-border text-muted-foreground"
      >
        Pick a track in the browser to load it
      </section>
    );
  }

  const { track } = detail;
  return (
    <section
      aria-label="Deck"
      className="flex h-full flex-col gap-3 border-b border-border px-4.5 py-3.5"
    >
      <div className="flex items-center gap-3.5">
        <div className="min-w-0">
          <h2 className="truncate text-base font-semibold">{track.title}</h2>
          <p className="truncate text-muted-foreground">{track.artist}</p>
        </div>
        <dl className="ml-auto flex gap-1.5">
          <Stat label="BPM" value={track.bpm?.toFixed(1) ?? "--"} highlight />
          <Stat label="Key" value={track.camelot ?? "--"} highlight />
          <Stat label="Energy" value={track.energy?.toString() ?? "--"} />
          <Stat label="LUFS" value={track.lufs?.toFixed(1) ?? "--"} />
        </dl>
      </div>
      <div className="flex items-center gap-3">
        <Button
          size="lg"
          onClick={() => void togglePlay()}
          aria-label={playing ? "Pause" : "Play"}
        >
          {playing ? <Pause aria-hidden="true" /> : <Play aria-hidden="true" />}
        </Button>
        <span className="font-mono text-xs tabular-nums">
          <span ref={timeEl}>0:00</span>{" "}
          <span className="text-muted-foreground">/ {formatTime(track.durationSec)}</span>
        </span>
        <span
          aria-hidden="true"
          className="h-1.5 w-32 overflow-hidden rounded-full bg-secondary"
        >
          <span ref={levelEl} className="block h-full w-0 bg-primary" />
        </span>
      </div>
    </section>
  );
}

function Stat({
  label,
  value,
  highlight,
}: {
  label: string;
  value: string;
  highlight?: boolean;
}): JSX.Element {
  return (
    <div className="min-w-18 rounded-lg border border-border bg-card px-2.5 py-1">
      <dt className="text-[11px] tracking-wider text-muted-foreground uppercase">
        {label}
      </dt>
      <dd
        className={
          highlight
            ? "font-mono text-[15px] text-primary tabular-nums"
            : "font-mono text-[15px] tabular-nums"
        }
      >
        {value}
      </dd>
    </div>
  );
}
