import type { JSX } from "react";

import { Transport } from "@/components/deck/Transport";
import { Waveform } from "@/components/deck/Waveform";
import { usePlayer } from "@/stores/player";

/** The deck: track header with stats, waveform with cue flags, transport. */
export function Deck(): JSX.Element {
  const detail = usePlayer((s) => s.detail);

  if (!detail) {
    return (
      <section
        aria-label="Deck"
        className="grid h-full place-items-center border-b border-border text-muted-foreground"
      >
        Double-click a track (or press Enter) to load it on the deck
      </section>
    );
  }

  const { track, analysis } = detail;
  return (
    <section
      aria-label="Deck"
      className="flex h-full min-h-0 flex-col gap-2.5 border-b border-border px-4.5 py-3"
    >
      <div className="flex items-center gap-3.5">
        <div
          aria-hidden="true"
          className="size-11 shrink-0 rounded-lg bg-linear-to-br from-ck-purple to-ck-pink"
        />
        <div className="min-w-0">
          <h2 className="truncate text-base font-semibold" title={track.title}>
            {track.title}
          </h2>
          <p className="truncate text-muted-foreground">{track.artist}</p>
        </div>
        <dl className="ml-auto flex shrink-0 gap-1.5">
          <Stat
            label="BPM"
            value={track.bpm?.toFixed(1) ?? "--"}
            highlight
            hint={
              analysis
                ? `confidence ${Math.round(analysis.bpmConfidence * 100)}%`
                : undefined
            }
          />
          <Stat
            label="Key"
            value={
              track.keyName?.replace(" minor", " min").replace(" major", " maj") ?? "--"
            }
          />
          <Stat label="Camelot" value={track.camelot ?? "--"} highlight />
          <Stat label="Energy" value={track.energy?.toString() ?? "--"} />
          <Stat label="LUFS" value={track.lufs?.toFixed(1) ?? "--"} />
        </dl>
      </div>
      <div className="min-h-16 flex-1">
        <Waveform detail={detail} />
      </div>
      <Transport detail={detail} />
    </section>
  );
}

interface StatProps {
  label: string;
  value: string;
  highlight?: boolean;
  hint?: string;
}

function Stat({ label, value, highlight, hint }: StatProps): JSX.Element {
  return (
    <div
      className="min-w-16 rounded-lg border border-border bg-card px-2.5 py-1"
      title={hint}
    >
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
