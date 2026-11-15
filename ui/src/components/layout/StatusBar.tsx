import { useEffect, useRef, useState, type JSX } from "react";

import { api } from "@/lib/ipc";
import { activeJobs, useJobs } from "@/stores/jobs";

/** Bottom bar: background jobs, offline promise, version. Device and backend arrive in PR B. */
export function StatusBar(): JSX.Element {
  const jobs = useJobs((s) => s.jobs);
  const running = activeJobs(jobs);
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    api()
      .appVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  const first = running[0];

  // Screen readers hear "started" and "done" only, never every progress tick.
  const [announcement, setAnnouncement] = useState("");
  const lastLabel = useRef<string | null>(null);
  const currentLabel = first?.label ?? null;
  useEffect(() => {
    if (currentLabel && currentLabel !== lastLabel.current)
      setAnnouncement(`${currentLabel} started`);
    else if (!currentLabel && lastLabel.current)
      setAnnouncement(`${lastLabel.current} done`);
    lastLabel.current = currentLabel;
  }, [currentLabel]);
  const progress =
    first && first.total > 0 ? Math.round((first.done / first.total) * 100) : 0;

  return (
    <footer className="flex h-7 shrink-0 items-center gap-4 border-t border-border bg-sidebar px-3.5 text-[11px] text-muted-foreground">
      {first ? (
        <span className="flex items-center gap-2">
          {first.label}
          <progress
            aria-label={first.label}
            max={100}
            value={progress}
            className="h-1 w-20 appearance-none overflow-hidden rounded-full [&::-moz-progress-bar]:bg-primary [&::-webkit-progress-bar]:bg-secondary [&::-webkit-progress-value]:bg-primary"
          />
          <span className="font-mono tabular-nums">
            {first.done} / {first.total}
          </span>
          {running.length > 1 && <span>+{running.length - 1} more</span>}
        </span>
      ) : (
        <span>No background jobs</span>
      )}
      <output className="sr-only" aria-live="polite">
        {announcement}
      </output>
      <span className="ml-auto">Offline · nothing uploaded</span>
      <span className="font-mono tabular-nums">{version ? `v${version}` : "v?"}</span>
    </footer>
  );
}
