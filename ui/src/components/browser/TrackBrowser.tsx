import type { JSX } from "react";

import { useLibrary } from "@/stores/library";
import { usePlayer } from "@/stores/player";

/**
 * Track browser (foundation version): a plain list to prove the data flow.
 * PR B replaces it with the virtualized, sortable TanStack table.
 */
export function TrackBrowser(): JSX.Element {
  const rows = useLibrary((s) => s.rows);
  const status = useLibrary((s) => s.status);
  const loadedId = usePlayer((s) => s.detail?.track.id ?? null);
  const load = usePlayer((s) => s.load);

  return (
    <section aria-label="Track browser" className="flex h-full flex-col">
      <header className="flex items-center gap-2.5 border-b border-border px-4.5 py-2.5">
        <h2 className="font-semibold">Tracks</h2>
        <span className="text-muted-foreground">
          {status === "loading" ? "Loading..." : `${rows.length} tracks`}
        </span>
      </header>
      <ul className="min-h-0 flex-1 overflow-y-auto">
        {rows.map((t) => (
          <li key={t.id}>
            <button
              type="button"
              onClick={() => void load(t.id)}
              aria-current={loadedId === t.id ? "true" : undefined}
              className="flex w-full items-center gap-3 border-b border-border/50 px-4.5 py-1.5 text-left hover:bg-card aria-[current=true]:bg-primary/10"
            >
              <span className="min-w-0 flex-1 truncate">{t.title}</span>
              <span className="w-28 truncate text-muted-foreground">{t.artist}</span>
              <span className="w-12 text-right font-mono tabular-nums">
                {t.bpm?.toFixed(1) ?? "--"}
              </span>
              <span className="w-10 font-mono text-primary tabular-nums">
                {t.camelot ?? "--"}
              </span>
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}
