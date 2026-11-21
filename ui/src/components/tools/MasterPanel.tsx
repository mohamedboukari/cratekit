import { Download, X } from "lucide-react";
import { useId, type JSX } from "react";
import { useTranslation } from "react-i18next";
import { toast } from "sonner";

import { Meter } from "@/components/tools/Meter";
import { ToolCard, ToolEmpty } from "@/components/tools/ToolCard";
import { Button } from "@/components/ui/button";
import { usePanelFocusFallback } from "@/hooks/usePanelFocusFallback";
import type { MasterPreset } from "@/lib/types";
import { activeJobs, useJobs } from "@/stores/jobs";
import { useMaster, type AbSide } from "@/stores/master";
import { usePlayer } from "@/stores/player";

/** Output never exceeds this (SPEC, Module 3); Rust enforces it, the UI shows it. */
const CEILING_DBTP = -1;
/** Loudness target range offered by the slider. */
const LUFS_MIN = -16;
const LUFS_MAX = -6;

/** A labelled native slider with its value shown beside the label. */
function RangeField({
  label,
  valueText,
  min,
  max,
  step,
  value,
  onChange,
}: {
  label: string;
  valueText: string;
  min: number;
  max: number;
  step: number;
  value: number;
  onChange: (value: number) => void;
}): JSX.Element {
  const id = useId();
  return (
    <div className="mt-3">
      <div className="flex justify-between">
        <label htmlFor={id}>{label}</label>
        <span aria-hidden="true" className="font-mono tabular-nums">
          {valueText}
        </span>
      </div>
      <input
        id={id}
        type="range"
        min={min}
        max={max}
        step={step}
        value={value}
        aria-valuetext={valueText}
        onChange={(e) => onChange(Number(e.target.value))}
        className="mt-1 w-full accent-primary"
      />
    </div>
  );
}

/** A row of toggle buttons where exactly one is pressed (presets, A/B, bit depth). */
function Segmented<T extends string | number>({
  legend,
  options,
  value,
  onChange,
}: {
  legend: string;
  options: ReadonlyArray<{ value: T; label: string; title?: string }>;
  value: T;
  onChange: (value: T) => void;
}): JSX.Element {
  return (
    <fieldset className="flex min-w-0 flex-wrap gap-1">
      <legend className="sr-only">{legend}</legend>
      {options.map((o) => (
        <Button
          key={o.value}
          variant={o.value === value ? "default" : "outline"}
          size="sm"
          aria-pressed={o.value === value}
          title={o.title}
          onClick={() => onChange(o.value)}
        >
          {o.label}
        </Button>
      ))}
    </fieldset>
  );
}

/** A/B sides, with translation keys for the label and the tooltip. */
const AB_OPTIONS = [
  { value: "a", label: "tools.master.ab.a", title: "tools.master.ab.aTitle" },
  { value: "b", label: "tools.master.ab.b", title: "tools.master.ab.bTitle" },
] as const satisfies ReadonlyArray<{ value: AbSide; label: string; title: string }>;

/** Export bit depths, with translation keys for the label and the tooltip. */
const BIT_DEPTHS = [
  { value: 24, label: "tools.master.bits24", title: "tools.master.bits24Title" },
  { value: 16, label: "tools.master.bits16", title: "tools.master.bits16Title" },
] as const satisfies ReadonlyArray<{ value: 16 | 24; label: string; title: string }>;

/** Presets the UI has translations for; Rust's English text covers any other. */
const KNOWN_PRESETS = ["club", "hip-hop", "pop", "lo-fi", "chill"] as const;
type KnownPreset = (typeof KNOWN_PRESETS)[number];

function isKnownPreset(name: string): name is KnownPreset {
  return KNOWN_PRESETS.some((known) => known === name);
}

/**
 * Master tab (#28): preset, intensity, loudness target, optional reference,
 * level-matched A/B, output meters and WAV export, which runs as a job with
 * progress and Cancel. Rust renders the B side and the export; the UI only
 * sends settings.
 */
export function MasterPanel(): JSX.Element {
  const { t } = useTranslation();
  const detail = usePlayer((s) => s.detail);
  const presets = useMaster((s) => s.presets);
  const presetsStatus = useMaster((s) => s.presetsStatus);
  const loadPresets = useMaster((s) => s.loadPresets);
  const settings = useMaster((s) => s.settings);
  const referenceTitle = useMaster((s) => s.referenceTitle);
  const side = useMaster((s) => s.side);
  const levelMatch = useMaster((s) => s.levelMatch);
  const exporting = useMaster((s) => s.exporting);
  const result = useMaster((s) => s.result);
  const setPreset = useMaster((s) => s.setPreset);
  const setIntensity = useMaster((s) => s.setIntensity);
  const setTargetLufs = useMaster((s) => s.setTargetLufs);
  const setReference = useMaster((s) => s.setReference);
  const setBitDepth = useMaster((s) => s.setBitDepth);
  const setSide = useMaster((s) => s.setSide);
  const setLevelMatch = useMaster((s) => s.setLevelMatch);
  const exportMaster = useMaster((s) => s.exportMaster);
  const cancelExport = useMaster((s) => s.cancelExport);
  const cancelling = useMaster((s) => s.cancelling);
  const exportTitle = useMaster((s) => s.exportTitle);
  // The running export (a `master` job, one at a time), its track and its progress, 0 to 100.
  const exportJobId = useJobs(
    (s) => activeJobs(s.jobs).find((j) => j.kind === "master")?.id ?? null,
  );
  const exportTrackId = useJobs((s) =>
    exportJobId === null ? null : (s.jobs[exportJobId]?.trackIds[0] ?? null),
  );
  const exportPct = useJobs((s) => {
    const job = exportJobId === null ? undefined : s.jobs[exportJobId];
    return job && job.total > 0 ? Math.round((job.done / job.total) * 100) : 0;
  });
  // Export swaps to progress and back: keep keyboard focus in the panel.
  usePanelFocusFallback(exportJobId === null ? "idle" : "exporting");

  if (!detail) return <ToolEmpty>{t("tools.master.empty")}</ToolEmpty>;

  const trackId = detail.track.id;
  const preset = presets.find((p) => p.name === settings.preset);
  const lastExport = result?.trackId === trackId ? result.master : null;
  const presetLabel = (p: MasterPreset): string =>
    isKnownPreset(p.name) ? t(`tools.master.presets.${p.name}.label`) : p.label;
  const presetDescription = (p: MasterPreset): string =>
    isKnownPreset(p.name)
      ? t(`tools.master.presets.${p.name}.description`)
      : p.description;

  const onExport = async (): Promise<void> => {
    const master = await exportMaster(trackId);
    if (master) toast.success(t("tools.master.exported"), { description: master.path });
  };

  return (
    <div className="flex flex-col gap-3 p-3">
      <ToolCard title={t("tools.master.sound")}>
        {presetsStatus === "ready" ? (
          <Segmented
            legend={t("tools.master.preset")}
            options={presets.map((p) => ({ value: p.name, label: presetLabel(p) }))}
            value={settings.preset}
            onChange={setPreset}
          />
        ) : presetsStatus === "loading" ? (
          <p className="text-muted-foreground">{t("tools.master.loadingPresets")}</p>
        ) : (
          <p className="flex items-center gap-2 text-muted-foreground">
            {t("tools.master.presetsError")}
            <Button variant="outline" size="sm" onClick={() => void loadPresets()}>
              {t("tools.master.retry")}
            </Button>
          </p>
        )}
        {preset && (
          <p className="mt-1.5 text-muted-foreground">{presetDescription(preset)}</p>
        )}

        <RangeField
          label={t("tools.master.intensity")}
          valueText={t("tools.percent", { value: Math.round(settings.intensity * 100) })}
          min={0}
          max={100}
          step={1}
          value={Math.round(settings.intensity * 100)}
          onChange={(v) => setIntensity(v / 100)}
        />
        <RangeField
          label={t("tools.master.loudnessTarget")}
          valueText={`${settings.targetLufs.toFixed(1)} LUFS`}
          min={LUFS_MIN}
          max={LUFS_MAX}
          step={0.5}
          value={settings.targetLufs}
          onChange={setTargetLufs}
        />

        <div className="mt-3 flex items-center gap-2">
          <span className="text-muted-foreground">{t("tools.master.reference")}</span>
          {referenceTitle ? (
            <>
              <span className="min-w-0 flex-1 truncate" title={referenceTitle}>
                {referenceTitle}
              </span>
              <Button
                variant="ghost"
                size="icon-xs"
                aria-label={t("tools.master.clearReference")}
                onClick={() => setReference(null)}
              >
                <X aria-hidden="true" />
              </Button>
            </>
          ) : (
            <span className="text-[11px] text-muted-foreground">
              {t("tools.master.noReference")}
            </span>
          )}
        </div>
        {settings.referenceTrackId === trackId && (
          <p className="mt-1 text-[11px] text-muted-foreground">
            {t("tools.master.referenceIsThisTrack")}
          </p>
        )}
      </ToolCard>

      <ToolCard
        title={t("tools.master.compare")}
        aside={
          <label className="flex items-center gap-1.5 text-[11px] text-muted-foreground">
            <input
              type="checkbox"
              checked={levelMatch}
              onChange={(e) => void setLevelMatch(e.target.checked)}
              className="accent-primary"
            />
            {t("tools.master.levelMatched")}
          </label>
        }
      >
        <Segmented
          legend={t("tools.master.abCompare")}
          options={AB_OPTIONS.map((o) => ({
            value: o.value,
            label: t(o.label),
            title: t(o.title),
          }))}
          value={side}
          onChange={setSide}
        />
        <div className="mt-3">
          <Meter ceilingDb={CEILING_DBTP} />
        </div>
      </ToolCard>

      <ToolCard title={t("tools.master.export")}>
        <p className="text-muted-foreground">
          {t("tools.master.target", {
            lufs: settings.targetLufs.toFixed(1),
            ceiling: CEILING_DBTP.toFixed(1),
          })}
        </p>
        {lastExport && (
          <p className="mt-1">
            <span className="text-primary">{t("tools.master.exportedLabel")}</span>{" "}
            <span className="font-mono tabular-nums">
              {`${lastExport.lufs.toFixed(1)} LUFS, ${lastExport.truePeakDb.toFixed(1)} dBTP`}
            </span>
            {!lastExport.targetReached && (
              <span className="block text-[11px] text-ck-orange">
                {t("tools.master.targetMissed", {
                  lufs: lastExport.targetLufs.toFixed(1),
                })}
              </span>
            )}
            <span
              className="block truncate text-[11px] text-muted-foreground"
              title={lastExport.path}
            >
              {lastExport.path}
            </span>
          </p>
        )}
        <div className="mt-2.5 flex flex-wrap items-center gap-2">
          <Segmented
            legend={t("tools.master.bitDepth")}
            options={BIT_DEPTHS.map((o) => ({
              value: o.value,
              label: t(o.label),
              title: t(o.title),
            }))}
            value={settings.bitDepth}
            onChange={setBitDepth}
          />
          {exportJobId !== null ? (
            <div className="ml-auto flex min-w-0 items-center gap-2">
              {/* Another track is on the deck: say which one is exporting. */}
              {exportTrackId !== trackId && exportTitle && (
                <span
                  className="min-w-0 max-w-40 truncate text-muted-foreground"
                  title={exportTitle}
                >
                  {t("tools.master.exportingTrack", { title: exportTitle })}
                </span>
              )}
              <progress
                max={100}
                value={exportPct}
                aria-label={t("tools.master.exportProgress")}
                className="h-1.5 w-24 appearance-none overflow-hidden rounded-full [&::-moz-progress-bar]:bg-primary [&::-webkit-progress-bar]:bg-secondary [&::-webkit-progress-value]:bg-primary"
              />
              <span className="w-10 text-right font-mono tabular-nums">
                {t("tools.percent", { value: exportPct })}
              </span>
              <Button variant="outline" onClick={() => void cancelExport(exportJobId)}>
                <X aria-hidden="true" data-icon="inline-start" />
                {t("tools.master.cancel")}
              </Button>
            </div>
          ) : (
            <Button
              className="ml-auto"
              disabled={exporting}
              onClick={() => void onExport()}
            >
              <Download aria-hidden="true" data-icon="inline-start" />
              {!exporting
                ? t("tools.master.exportWav")
                : cancelling
                  ? t("tools.master.cancelling")
                  : t("tools.master.rendering")}
            </Button>
          )}
        </div>
      </ToolCard>
    </div>
  );
}
