import { cn } from "cn";
import { RefreshCw } from "lucide-react";
import type { JSX } from "react";
import { useTranslation } from "react-i18next";

import { Stat, ToolCard, ToolEmpty } from "@/components/tools/ToolCard";
import { Button } from "@/components/ui/button";
import { usePanelFocusFallback } from "@/hooks/usePanelFocusFallback";
import { i18n } from "@/i18n";
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

function analyze(detail: TrackDetail): void {
  api()
    .analyzeTracks([detail.track.id])
    .catch((error: unknown) => notifyError(error, i18n.t("tools.analyze.startError")));
}

/**
 * Analyze tab (#18, UI side): key and Camelot, chroma, mixes-well-with chips
 * that filter the browser, tempo, cue points that seek, and loudness. Every
 * value comes from Rust's analysis; nothing is computed here.
 */
export function AnalyzePanel(): JSX.Element {
  const { t } = useTranslation();
  const detail = usePlayer((s) => s.detail);
  const trackId = detail?.track.id ?? null;
  // Progress (0 to 100) of an analysis that includes the loaded track, or null.
  const running = useJobs((s) => {
    if (trackId === null) return null;
    const job = activeJobs(s.jobs).find(
      (j) => j.kind === "analysis" && j.trackIds.includes(trackId),
    );
    if (!job) return null;
    return job.total > 0 ? Math.round((job.done / job.total) * 100) : 0;
  });
  usePanelFocusFallback(!detail ? "empty" : detail.analysis ? "analyzed" : "unanalyzed");

  if (!detail) return <ToolEmpty>{t("tools.analyze.empty")}</ToolEmpty>;
  if (!detail.analysis) {
    return (
      <ToolEmpty>
        <p>{t("tools.analyze.notAnalyzed", { title: detail.track.title })}</p>
        <Button disabled={running !== null} onClick={() => analyze(detail)}>
          {running === null
            ? t("tools.analyze.analyzeNow")
            : t("tools.analyze.analyzing", {
                progress: t("tools.percent", { value: running }),
              })}
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
  /** Progress (0 to 100) of a re-analysis in flight, or null. */
  running: number | null;
  analysis: Analysis;
}): JSX.Element {
  const { t } = useTranslation();
  const seek = usePlayer((s) => s.seek);
  const setQuery = useLibrary((s) => s.setQuery);
  const strongest = Math.max(...a.chroma, 0.0001);

  return (
    <div className="flex flex-col gap-3 p-3">
      <ToolCard
        title={t("tools.analyze.key")}
        aside={
          <span className="text-[11px] text-muted-foreground">
            {t("tools.analyze.sure", { percent: Math.round(a.keyConfidence * 100) })}
          </span>
        }
      >
        <div className="flex items-baseline gap-3">
          <span className="font-mono text-4xl font-bold text-primary">{a.camelot}</span>
          <span className="text-base">{a.keyName}</span>
        </div>
        <figure className="mt-3">
          <figcaption className="sr-only">
            {t("tools.analyze.pitchStrength", {
              notes: NOTE_NAMES.filter((_, i) => a.chromaInKey[i]).join(", "),
            })}
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
            {t("tools.analyze.mixesWellWith")}
          </h4>
          <div className="flex flex-wrap gap-1.5">
            {a.mixesWellWith.map((code) => (
              <Button
                key={code}
                variant="outline"
                size="sm"
                className="rounded-full font-mono"
                onClick={() => void setQuery({ matchCamelot: code })}
                aria-label={t("tools.analyze.showMatching", { code })}
              >
                {code}
              </Button>
            ))}
          </div>
        </div>
      </ToolCard>

      <ToolCard
        title={t("tools.analyze.tempoAndCues")}
        aside={
          <span className="text-[11px] text-muted-foreground">
            {t("tools.analyze.sure", { percent: Math.round(a.bpmConfidence * 100) })}
          </span>
        }
      >
        <dl>
          <Stat label="BPM" value={a.bpm.toFixed(1)} />
          <Stat
            label={t("tools.analyze.firstDownbeat")}
            value={`${a.firstBeatSec.toFixed(3)} s`}
          />
          <Stat label={t("tools.analyze.energy")} value={`${a.energy} / 10`} />
          <Stat
            label={t("tools.analyze.drop")}
            value={
              a.dropSec === null ? (
                "--"
              ) : (
                <CueLink
                  sec={a.dropSec}
                  label={t("tools.analyze.jumpToDrop", { time: formatTime(a.dropSec) })}
                  onSeek={seek}
                />
              )
            }
          />
          <Stat
            label={t("tools.analyze.mixOut")}
            value={
              a.mixOutSec === null ? (
                "--"
              ) : (
                <CueLink
                  sec={a.mixOutSec}
                  label={t("tools.analyze.jumpToMixOut", {
                    time: formatTime(a.mixOutSec),
                  })}
                  onSeek={seek}
                />
              )
            }
          />
        </dl>
      </ToolCard>

      <ToolCard title={t("tools.analyze.loudness")}>
        <dl>
          <Stat
            label={t("tools.analyze.integrated")}
            value={`${a.lufs.toFixed(1)} LUFS`}
          />
          <Stat
            label={t("tools.analyze.truePeak")}
            value={`${a.truePeakDb.toFixed(1)} dBTP`}
          />
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
        {running === null
          ? t("tools.analyze.reanalyze")
          : t("tools.analyze.analyzing", {
              progress: t("tools.percent", { value: running }),
            })}
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
  /** Accessible name, e.g. "Jump to the drop at 1:02". */
  label: string;
  onSeek: (sec: number) => Promise<void>;
}): JSX.Element {
  return (
    <button
      type="button"
      onClick={() => void onSeek(sec)}
      aria-label={label}
      className="rounded-sm text-primary underline-offset-2 hover:underline"
    >
      {formatTime(sec)}
    </button>
  );
}
