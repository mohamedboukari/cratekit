import { cn } from "cn";
import type { JSX } from "react";

import { useLibrary } from "@/stores/library";

const SWATCH: Record<string, string> = {
  orange: "bg-ck-orange",
  blue: "bg-ck-blue",
  pink: "bg-ck-pink",
  purple: "bg-ck-purple",
  lime: "bg-ck-lime",
  yellow: "bg-ck-yellow",
  red: "bg-ck-red",
  teal: "bg-ck-teal",
};

/** Library counts and crates. Filled out further in PR B (devices, tools). */
export function Sidebar(): JSX.Element {
  const crates = useLibrary((s) => s.crates);
  const counts = useLibrary((s) => s.counts);
  const crateId = useLibrary((s) => s.query.crateId);
  const selectCrate = useLibrary((s) => s.selectCrate);

  return (
    <nav
      aria-label="Library"
      className="flex h-full flex-col gap-4 overflow-y-auto bg-sidebar px-2.5 py-3.5"
    >
      <div className="flex items-center gap-2 px-1.5 font-heading text-[15px] font-bold">
        <span
          aria-hidden="true"
          className="grid size-6 place-items-center rounded-md bg-primary text-primary-foreground"
        >
          ◉
        </span>
        Cratekit
      </div>

      <section>
        <h2 className="px-2 pb-1.5 text-[11px] font-semibold tracking-wider text-muted-foreground uppercase">
          Library
        </h2>
        <SidebarItem
          label="All tracks"
          count={counts?.all}
          active={crateId === null}
          onSelect={() => void selectCrate(null)}
        />
        <SidebarItem label="Needs analysis" count={counts?.needsAnalysis} disabled />
      </section>

      <section>
        <h2 className="px-2 pb-1.5 text-[11px] font-semibold tracking-wider text-muted-foreground uppercase">
          Crates
        </h2>
        {crates.map((c) => (
          <SidebarItem
            key={c.id}
            label={c.name}
            count={c.trackCount}
            swatch={SWATCH[c.color]}
            active={crateId === c.id}
            onSelect={() => void selectCrate(c.id)}
          />
        ))}
      </section>
    </nav>
  );
}

interface SidebarItemProps {
  label: string;
  count?: number;
  swatch?: string;
  active?: boolean;
  disabled?: boolean;
  onSelect?: () => void;
}

function SidebarItem({
  label,
  count,
  swatch,
  active,
  disabled,
  onSelect,
}: SidebarItemProps): JSX.Element {
  return (
    <button
      type="button"
      onClick={onSelect}
      disabled={disabled}
      aria-current={active ? "true" : undefined}
      className={cn(
        "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-[13px] text-foreground/80 hover:bg-card disabled:opacity-50",
        active && "bg-secondary text-foreground",
      )}
    >
      {swatch && <span aria-hidden="true" className={cn("size-2 rounded-xs", swatch)} />}
      <span className="truncate">{label}</span>
      {count !== undefined && (
        <span className="ml-auto font-mono text-[11px] text-muted-foreground tabular-nums">
          {count}
        </span>
      )}
    </button>
  );
}
