import { Pause, Play, X } from "lucide-react";
import { useEffect, useState, type JSX } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { backendLabel, jobLabel } from "@/lib/format";
import { api } from "@/lib/ipc";
import type { Job } from "@/lib/types";
import { useDevices } from "@/stores/devices";
import { activeJobs, useJobs } from "@/stores/jobs";

function percent(job: Job): number {
  return job.total > 0 ? Math.round((job.done / job.total) * 100) : 0;
}

/** Bottom bar: controller, background jobs (with the queue), backend, offline promise, version. */
export function StatusBar(): JSX.Element {
  const { t } = useTranslation();
  const jobs = useJobs((s) => s.jobs);
  const paused = useJobs((s) => s.paused);
  const cancel = useJobs((s) => s.cancel);
  const setPaused = useJobs((s) => s.setPaused);
  // Screen readers hear "started" and "done" only, never every progress tick.
  const announcement = useJobs((s) => s.announcement);
  const controller = useDevices((s) => s.midi.find((d) => d.connected) ?? null);
  const backend = useDevices((s) => s.backend);
  const running = activeJobs(jobs);
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    api()
      .appVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  const first = running[0];

  return (
    <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-border bg-sidebar px-3.5 text-[11px] text-muted-foreground">
      <span className="flex items-center gap-1.5">
        <span
          aria-hidden="true"
          className={
            controller
              ? "size-1.5 rounded-full bg-primary"
              : "size-1.5 rounded-full bg-secondary"
          }
        />
        {controller ? controller.name : t("layout.statusBar.noController")}
      </span>

      {/* Always rendered, so focus has a home when the last job ends while the queue is open. */}
      <Popover>
        <PopoverTrigger
          render={
            <button
              type="button"
              className="flex items-center gap-2 rounded-sm px-1 hover:text-foreground"
              aria-label={
                first
                  ? t("layout.statusBar.jobsLabel", { running: running.length })
                  : t("layout.statusBar.noJobsLabel")
              }
            />
          }
        >
          {first ? (
            <>
              {jobLabel(first)}
              <progress
                aria-hidden="true"
                max={100}
                value={percent(first)}
                className="h-1 w-20 appearance-none overflow-hidden rounded-full [&::-moz-progress-bar]:bg-primary [&::-webkit-progress-bar]:bg-secondary [&::-webkit-progress-value]:bg-primary"
              />
              <span className="font-mono tabular-nums">
                {first.done} / {first.total}
              </span>
              {running.length > 1 && (
                <span>{t("layout.statusBar.more", { count: running.length - 1 })}</span>
              )}
            </>
          ) : (
            t("layout.statusBar.noJobs")
          )}
          {paused && (
            <span className="text-ck-orange">{t("layout.statusBar.paused")}</span>
          )}
        </PopoverTrigger>
        <PopoverContent side="top" align="start" className="w-80">
          <div className="mb-2 flex items-center justify-between">
            <h3 className="text-xs font-semibold">{t("layout.statusBar.jobs")}</h3>
            <Button variant="ghost" size="sm" onClick={() => void setPaused(!paused)}>
              {paused ? (
                <Play aria-hidden="true" data-icon="inline-start" />
              ) : (
                <Pause aria-hidden="true" data-icon="inline-start" />
              )}
              {paused ? t("layout.statusBar.resume") : t("layout.statusBar.pause")}
            </Button>
          </div>
          {running.length === 0 ? (
            <p className="text-xs text-muted-foreground">
              {t("layout.statusBar.nothingRunning")}
            </p>
          ) : (
            <ul className="flex flex-col gap-2">
              {running.map((job) => (
                <li key={job.id} className="flex items-center gap-2 text-xs">
                  <span className="min-w-0 flex-1 truncate">{jobLabel(job)}</span>
                  <span className="font-mono text-muted-foreground tabular-nums">
                    {percent(job)}%
                  </span>
                  <Button
                    variant="ghost"
                    size="icon-xs"
                    aria-label={t("layout.statusBar.cancelJob", { label: jobLabel(job) })}
                    onClick={() => void cancel(job.id)}
                  >
                    <X aria-hidden="true" />
                  </Button>
                </li>
              ))}
            </ul>
          )}
        </PopoverContent>
      </Popover>
      <output className="sr-only" aria-live="polite">
        {announcement}
      </output>

      <span className="ml-auto">
        {backend ? backendLabel(backend.active) : t("layout.statusBar.backendChecking")}
      </span>
      <span>{t("layout.statusBar.offline")}</span>
      <span className="font-mono tabular-nums">{version ? `v${version}` : "v?"}</span>
    </footer>
  );
}
