import { cn } from "cn";
import type { JSX } from "react";

import { CUE_BG } from "@/components/deck/cueColors";
import { useDevices } from "@/stores/devices";
import { useLibrary } from "@/stores/library";

/** Library counts, crates and the connected controller. */
export function Sidebar(): JSX.Element {
  const crates = useLibrary((s) => s.crates);
  const counts = useLibrary((s) => s.counts);
  const crateId = useLibrary((s) => s.query.crateId);
  const selectCrate = useLibrary((s) => s.selectCrate);
  const controller = useDevices((s) => s.midi.find((d) => d.connected) ?? null);

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
            swatch={CUE_BG[c.color]}
            active={crateId === c.id}
            onSelect={() => void selectCrate(c.id)}
          />
        ))}
      </section>

      <section className="mt-auto">
        <h2 className="px-2 pb-1.5 text-[11px] font-semibold tracking-wider text-muted-foreground uppercase">
          Devices
        </h2>
        <div className="mx-1 rounded-lg border border-border bg-card p-2.5">
          {controller ? (
            <>
              <b className="block text-xs">{controller.name}</b>
              <span className="inline-flex items-center gap-1.5 text-[11px] text-primary">
                <span aria-hidden="true" className="size-1.5 rounded-full bg-primary" />
                connected
              </span>
            </>
          ) : (
            <span className="text-[11px] text-muted-foreground">
              Plug in a controller or use keyboard shortcuts
            </span>
          )}
        </div>
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
