import type { JSX } from "react";

/** Shown only in mock mode (browser dev server and the GitHub Pages demo). */
export function DemoBanner(): JSX.Element {
  return (
    <div
      role="note"
      className="shrink-0 bg-primary px-3 py-1 text-center text-[11px] font-semibold text-primary-foreground"
    >
      Demo with fake data. The desktop app analyzes your own tracks, offline.
    </div>
  );
}
