import { cn } from "cn";
import { Download, Scissors, X } from "lucide-react";
import { useEffect, useRef, useState, type JSX } from "react";
import { toast } from "sonner";

import { drawMiniWaveform } from "@/canvas/waveform";
import { ModelDownloadDialog } from "@/components/dialogs/ModelDownloadDialog";
import { ToolCard, ToolEmpty } from "@/components/tools/ToolCard";
import { Button } from "@/components/ui/button";
import { usePanelFocusFallback } from "@/hooks/usePanelFocusFallback";
import { BACKEND_LABEL } from "@/lib/format";
import type { Job, StemName, Waveform } from "@/lib/types";
import { useDevices } from "@/stores/devices";
import { activeJobs, useJobs } from "@/stores/jobs";
import { usePlayer } from "@/stores/player";
import { ALL_STEMS, type StemLayout, useStems } from "@/stores/stems";

/** Stem colours: the token for the dot, and the CSS variable for the canvas. */
const STEM_STYLE: Record<
  StemName,
  { dot: string; token: string; label: string; key: string }
> = {
  vocals: { dot: "bg-ck-pink", token: "--color-ck-pink", label: "Vocals", key: "Q" },
  drums: { dot: "bg-ck-orange", token: "--color-ck-orange", label: "Drums", key: "W" },
  bass: { dot: "bg-ck-blue", token: "--color-ck-blue", label: "Bass", key: "E" },
  other: { dot: "bg-ck-teal", token: "--color-ck-teal", label: "Other", key: "R" },
};

interface StemRow {
  id: string;
  label: string;
  stems: readonly StemName[];
  /** The stem whose colour represents the row. */
  lead: StemName;
  /** Which waveform Rust sent for this row. */
  wave: StemName | "instrumental";
  keyHint: string | null;
}

/** Four rows, or vocals plus instrumental (drums, bass and other together). */
function rowsFor(layout: StemLayout): StemRow[] {
  if (layout === "two") {
    return [
      {
        id: "vocals",
        label: "Vocals",
        stems: ["vocals"],
        lead: "vocals",
        wave: "vocals",
        keyHint: "Q",
      },
      {
        id: "instrumental",
        label: "Instrumental",
        stems: ["drums", "bass", "other"],
        lead: "drums",
        wave: "instrumental",
        keyHint: null,
      },
    ];
  }
  return ALL_STEMS.map((stem) => ({
    id: stem,
    label: STEM_STYLE[stem].label,
    stems: [stem],
    lead: stem,
    wave: stem,
    keyHint: STEM_STYLE[stem].key,
  }));
}

function megabytes(bytes: number): string {
  return `${Math.round(bytes / 1_000_000)} MB`;
}

/**
 * Stems tab (#35, UI side): model download, split with progress, and a stem
 * mixer (mute, solo, volume) in four or two stems. Rust splits and mixes;
 * this panel sends the controls and shows what Rust reports.
 */
export function StemsPanel(): JSX.Element {
  const detail = usePlayer((s) => s.detail);
  const info = useStems((s) => s.info);
  const status = useStems((s) => s.status);
  const modelReady = useStems((s) => s.modelReady);
  const modelSize = useStems((s) => s.modelSizeBytes);
  const layout = useStems((s) => s.layout);
  const load = useStems((s) => s.load);
  const split = useStems((s) => s.split);
  const downloadModel = useStems((s) => s.downloadModel);
  const setLayout = useStems((s) => s.setLayout);
  const exportStems = useStems((s) => s.exportStems);
  const backend = useDevices((s) => s.backend);
  const jobs = useJobs((s) => s.jobs);
  const cancel = useJobs((s) => s.cancel);

  const trackId = detail?.track.id ?? null;
  useEffect(() => {
    if (trackId !== null) void load(trackId);
  }, [trackId, load]);

  // Only jobs for the loaded track drive this tab (another track can split in the background).
  const running = activeJobs(jobs);
  const splitJob =
    trackId === null
      ? undefined
      : running.find((j) => j.kind === "stems" && j.trackIds.includes(trackId));
  const downloadJob = running.find((j) => j.kind === "model-download");
  const view = !detail
    ? "empty"
    : info
      ? "mixer"
      : splitJob
        ? "progress"
        : status !== "ready"
          ? "checking"
          : modelReady === false
            ? "model"
            : "split";
  usePanelFocusFallback(view);

  if (!detail || trackId === null)
    return <ToolEmpty>Load a track to split it into stems.</ToolEmpty>;

  const backendLabel = backend ? BACKEND_LABEL[backend.active] : null;

  if (view === "progress" && splitJob) {
    const pct =
      splitJob.total > 0 ? Math.round((splitJob.done / splitJob.total) * 100) : 0;
    return (
      <ToolEmpty>
        <p>{`${splitJob.label}: ${detail.track.title}${backendLabel ? ` on ${backendLabel}` : ""}`}</p>
        <progress
          max={100}
          value={pct}
          aria-label="Split progress"
          className="h-1.5 w-48 appearance-none overflow-hidden rounded-full [&::-moz-progress-bar]:bg-primary [&::-webkit-progress-bar]:bg-secondary [&::-webkit-progress-value]:bg-primary"
        />
        <span className="font-mono tabular-nums">{pct}%</span>
        <Button variant="outline" size="sm" onClick={() => void cancel(splitJob.id)}>
          <X aria-hidden="true" data-icon="inline-start" />
          Cancel
        </Button>
      </ToolEmpty>
    );
  }

  if (!info) {
    if (status !== "ready") return <ToolEmpty>Checking stems...</ToolEmpty>;
    if (modelReady === false)
      return (
        <ModelRequired
          sizeBytes={modelSize}
          job={downloadJob}
          onStart={() => void downloadModel()}
          onCancelJob={(job) => void cancel(job.id)}
        />
      );
    return (
      <ToolEmpty>
        <p>Split {detail.track.title} into vocals, drums, bass and other.</p>
        <Button onClick={() => void split(trackId)}>
          <Scissors aria-hidden="true" data-icon="inline-start" />
          Split stems
        </Button>
        {backendLabel && <span className="text-[11px]">Runs on {backendLabel}</span>}
      </ToolEmpty>
    );
  }

  const onExport = async (): Promise<void> => {
    const paths = await exportStems(trackId);
    if (paths) toast.success(`Exported ${paths.length} stems`, { description: paths[0] });
  };

  return (
    <div className="flex flex-col gap-3 p-3">
      <ToolCard
        title="Stems"
        aside={
          <span className="text-[11px] text-muted-foreground">
            {`Split in ${info.splitSec.toFixed(1)} s, cached${backendLabel ? `, ${backendLabel}` : ""}`}
          </span>
        }
      >
        <fieldset className="mb-2.5 flex gap-1">
          <legend className="sr-only">Stem layout</legend>
          {(["four", "two"] as const).map((l) => (
            <Button
              key={l}
              size="sm"
              variant={layout === l ? "default" : "outline"}
              aria-pressed={layout === l}
              onClick={() => setLayout(l)}
            >
              {l === "four" ? "4 stems" : "2 stems"}
            </Button>
          ))}
        </fieldset>
        <ul className="flex flex-col gap-2">
          {rowsFor(layout).map((row) => (
            <StemMixerRow
              key={row.id}
              row={row}
              waveform={
                row.wave === "instrumental" ? info.instrumental : info.waveforms[row.wave]
              }
            />
          ))}
        </ul>
      </ToolCard>
      <Button
        variant="outline"
        size="sm"
        className="self-start"
        onClick={() => void onExport()}
      >
        <Download aria-hidden="true" data-icon="inline-start" />
        Export stems (WAV)
      </Button>
    </div>
  );
}

/** How much of a row's group is on: every stem, some (e.g. W in the 2-stem layout), or none. */
type GroupState = "all" | "some" | "none";

function groupState(
  flags: Record<StemName, boolean>,
  stems: readonly StemName[],
): GroupState {
  const on = stems.filter((stem) => flags[stem]).length;
  return on === 0 ? "none" : on === stems.length ? "all" : "some";
}

function pressedFor(state: GroupState): boolean | "mixed" {
  return state === "some" ? "mixed" : state === "all";
}

function StemMixerRow({
  row,
  waveform,
}: {
  row: StemRow;
  waveform: Waveform;
}): JSX.Element {
  const lead = row.stems[0] ?? row.lead;
  const gain = useStems((s) => s.gains[lead]);
  const gainsDiffer = useStems((s) =>
    row.stems.some((stem) => s.gains[stem] !== s.gains[lead]),
  );
  const muted = useStems((s) => groupState(s.muted, row.stems));
  const soloed = useStems((s) => groupState(s.soloed, row.stems));
  const setGain = useStems((s) => s.setGain);
  const toggleMute = useStems((s) => s.toggleMute);
  const toggleSolo = useStems((s) => s.toggleSolo);
  const style = STEM_STYLE[row.lead];
  const volumeText = gainsDiffer ? "mixed" : `${Math.round(gain * 100)}%`;

  // Two lines, so the row fits the narrowest tool panel: controls, then the waveform.
  return (
    <li className={cn("flex flex-col gap-1", muted === "all" && "opacity-50")}>
      <div className="flex items-center gap-2">
        <span
          aria-hidden="true"
          className={cn("size-2 shrink-0 rounded-full", style.dot)}
        />
        <span className="min-w-0 flex-1 truncate">{row.label}</span>
        <Button
          variant={muted === "none" ? "outline" : "default"}
          size="icon-sm"
          aria-pressed={pressedFor(muted)}
          aria-label={`Mute ${row.label}`}
          title={row.keyHint ? `Mute (${row.keyHint})` : "Mute"}
          onClick={() => void toggleMute(row.stems)}
          className={cn(muted === "some" && "opacity-70")}
        >
          M
        </Button>
        <Button
          variant={soloed === "none" ? "outline" : "default"}
          size="icon-sm"
          aria-pressed={pressedFor(soloed)}
          aria-label={`Solo ${row.label}`}
          title="Solo"
          onClick={() => void toggleSolo(row.stems)}
          className={cn(soloed === "some" && "opacity-70")}
        >
          S
        </Button>
        <input
          type="range"
          min={0}
          max={100}
          value={Math.round(gain * 100)}
          aria-label={`${row.label} volume`}
          aria-valuetext={volumeText}
          title={`Volume ${volumeText}`}
          onChange={(e) => void setGain(row.stems, Number(e.target.value) / 100)}
          className="w-20 shrink-0 accent-primary"
        />
      </div>
      <StemWave waveform={waveform} colorToken={style.token} />
    </li>
  );
}

/** Static waveform of one stem, drawn once per size or data change. */
function StemWave({
  waveform,
  colorToken,
}: {
  waveform: Waveform;
  colorToken: string;
}): JSX.Element {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const canvas = canvasRef.current;
    const ctx = canvas?.getContext("2d");
    if (!canvas || !ctx) return;
    const draw = (): void => {
      const dpr = window.devicePixelRatio || 1;
      canvas.width = Math.max(1, Math.round(canvas.clientWidth * dpr));
      canvas.height = Math.max(1, Math.round(canvas.clientHeight * dpr));
      const css = getComputedStyle(document.documentElement);
      const color = css.getPropertyValue(colorToken).trim() || "#858c96";
      drawMiniWaveform(ctx, canvas.width, canvas.height, waveform, {
        background: css.getPropertyValue("--color-ck-panel").trim() || "#15181d",
        peak: color,
        rms: "rgba(12, 14, 17, 0.35)",
      });
    };
    const observer = new ResizeObserver(draw);
    observer.observe(canvas);
    draw();
    return () => observer.disconnect();
  }, [waveform, colorToken]);
  return <canvas ref={canvasRef} aria-hidden="true" className="h-6 w-full rounded-sm" />;
}

/**
 * The model is missing: explain, and offer the download dialog. Its open state
 * lives here, so it resets when the model arrives and this view goes away.
 */
function ModelRequired({
  sizeBytes,
  job,
  onStart,
  onCancelJob,
}: {
  sizeBytes: number | null;
  job: Job | undefined;
  onStart: () => void;
  onCancelJob: (job: Job) => void;
}): JSX.Element {
  const [open, setOpen] = useState(false);
  return (
    <ToolEmpty>
      <p>
        Splitting stems needs the stems model
        {sizeBytes !== null ? ` (${megabytes(sizeBytes)})` : ""}. It downloads once and
        stays on this computer.
      </p>
      <Button onClick={() => setOpen(true)}>
        <Download aria-hidden="true" data-icon="inline-start" />
        {job ? "Downloading..." : "Download model"}
      </Button>
      <ModelDownloadDialog
        open={open}
        sizeBytes={sizeBytes}
        job={job}
        onStart={onStart}
        onCancelJob={onCancelJob}
        onClose={() => setOpen(false)}
      />
    </ToolEmpty>
  );
}
