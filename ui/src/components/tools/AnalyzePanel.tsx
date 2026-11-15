import { cn } from "cn";
import { RefreshCw } from "lucide-react";
import type { JSX } from "react";

import { Stat, ToolCard, ToolEmpty } from "@/components/tools/ToolCard";
import { Button } from "@/components/ui/button";
import { usePanelFocusFallback } from "@/hooks/usePanelFocusFallback";
import { api } from "@/lib/ipc";
import { formatTime } from "@/lib/format";
import { notifyError } from "@/lib/notify";
import type { Analysis, TrackDetail } from "@/lib/types";
import { activeJobs, useJobs } from "@/stores/jobs";
import { useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";

/** Pitch class names for the chroma bars (display labels only). */
const NOTE_NAMES = [
  "C",
  "C#",
  "D",
  "D#",
  "E",
  "F",
  "F#",
  "G",
  "G#",
  "A",
  "A#",
  "B",
] as const;

function percent(value: number): string {
  return `${Math.round(value * 100)}%`;
}

function analyze(detail: TrackDetail): void {
  api()
    .analyzeTracks([detail.track.id])
    .catch((error: unknown) => notifyError(error, "Could not start the analysis"));
}

/**
 * Analyze tab (#18, UI side): key and Camelot, chroma, mixes-well-with chips
 * that filter the browser, tempo, cue points that seek, and loudness. Every
 * value comes from Rust's analysis; nothing is computed here.
 */
export function AnalyzePanel(): JSX.Element {
  const detail = usePlayer((s) => s.detail);
  const trackId = detail?.track.id ?? null;
  // Progress of an analysis that includes the loaded track, as "n%", or null.
  const running = useJobs((s) => {
    if (trackId === null) return null;
    const job = activeJobs(s.jobs).find(
      (j) => j.kind === "analysis" && j.trackIds.includes(trackId),
    );
    if (!job) return null;
    return `${job.total > 0 ? Math.round((job.done / job.total) * 100) : 0}%`;
  });
  usePanelFocusFallback(!detail ? "empty" : detail.analysis ? "analyzed" : "unanalyzed");

  if (!detail)
    return <ToolEmpty>Load a track to see its key, tempo and loudness.</ToolEmpty>;
  if (!detail.analysis) {
    return (
      <ToolEmpty>
        <p>{detail.track.title} is not analyzed yet.</p>
        <Button disabled={running !== null} onClick={() => analyze(detail)}>
          {running === null ? "Analyze now" : `Analyzing... ${running}`}
        </Button>
      </ToolEmpty>
    );
  }
  return <AnalysisView detail={detail} analysis={detail.analysis} running={running} />;
}

function AnalysisView({
  detail,
  analysis: a,
  running,
}: {
  detail: TrackDetail;
  /** Progress of a re-analysis in flight, or null. */
  running: string | null;
  analysis: Analysis;
}): JSX.Element {
  const seek = usePlayer((s) => s.seek);
  const setQuery = useLibrary((s) => s.setQuery);
  const strongest = Math.max(...a.chroma, 0.0001);

  return (
    <div className="flex flex-col gap-3 p-3">
      <ToolCard
        title="Key"
        aside={
          <span className="text-[11px] text-muted-foreground">
            {percent(a.keyConfidence)} sure
          </span>
        }
      >
        <div className="flex items-baseline gap-3">
          <span className="font-mono text-4xl font-bold text-primary">{a.camelot}</span>
          <span className="text-base">{a.keyName}</span>
        </div>
        <figure className="mt-3">
          <figcaption className="sr-only">
            {`Pitch strength. Notes in the key: ${NOTE_NAMES.filter((_, i) => a.chromaInKey[i]).join(", ")}`}
          </figcaption>
          <div aria-hidden="true" className="flex h-16 items-end gap-1">
            {a.chroma.map((value, i) => (
              <div
                key={NOTE_NAMES[i]}
                className="flex flex-1 flex-col items-center gap-1"
              >
                <div
                  className={cn(
                    "w-full rounded-t-sm",
                    a.chromaInKey[i] ? "bg-primary" : "bg-secondary",
                  )}
                  style={{ height: `${Math.max(4, (value / strongest) * 48)}px` }}
                />
                <span className="font-mono text-[9px] text-muted-foreground">
                  {NOTE_NAMES[i]}
                </span>
              </div>
            ))}
          </div>
        </figure>
        <div className="mt-3">
          <h4 className="mb-1.5 text-[11px] text-muted-foreground">
            Mixes well with (click to filter the browser)
          </h4>
          <div className="flex flex-wrap gap-1.5">
            {a.mixesWellWith.map((code) => (
              <Button
                key={code}
                variant="outline"
                size="sm"
                className="rounded-full font-mono"
                onClick={() => void setQuery({ matchCamelot: code })}
                aria-label={`Show tracks that match ${code}`}
              >
                {code}
              </Button>
            ))}
          </div>
        </div>
      </ToolCard>

      <ToolCard
        title="Tempo and cues"
        aside={
          <span className="text-[11px] text-muted-foreground">
            {percent(a.bpmConfidence)} sure
          </span>
        }
      >
        <dl>
          <Stat label="BPM" value={a.bpm.toFixed(1)} />
          <Stat label="First downbeat" value={`${a.firstBeatSec.toFixed(3)} s`} />
          <Stat label="Energy" value={`${a.energy} / 10`} />
          <Stat
            label="Drop"
            value={
              a.dropSec === null ? (
                "--"
              ) : (
                <CueLink sec={a.dropSec} label="drop" onSeek={seek} />
              )
            }
          />
          <Stat
            label="Mix out"
            value={
              a.mixOutSec === null ? (
                "--"
              ) : (
                <CueLink sec={a.mixOutSec} label="mix-out point" onSeek={seek} />
              )
            }
          />
        </dl>
      </ToolCard>

      <ToolCard title="Loudness">
        <dl>
          <Stat label="Integrated" value={`${a.lufs.toFixed(1)} LUFS`} />
          <Stat label="True peak" value={`${a.truePeakDb.toFixed(1)} dBTP`} />
        </dl>
      </ToolCard>

      <Button
        variant="outline"
        size="sm"
        className="self-start"
        disabled={running !== null}
        onClick={() => analyze(detail)}
      >
        <RefreshCw aria-hidden="true" data-icon="inline-start" />
        {running === null ? "Re-analyze" : `Analyzing... ${running}`}
      </Button>
    </div>
  );
}

function CueLink({
  sec,
  label,
  onSeek,
}: {
  sec: number;
  label: string;
  onSeek: (sec: number) => Promise<void>;
}): JSX.Element {
  return (
    <button
      type="button"
      onClick={() => void onSeek(sec)}
      aria-label={`Jump to the ${label} at ${formatTime(sec)}`}
      className="rounded-sm text-primary underline-offset-2 hover:underline"
    >
      {formatTime(sec)}
    </button>
  );
}
