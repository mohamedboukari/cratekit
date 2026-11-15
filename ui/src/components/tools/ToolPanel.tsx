import { cn } from "cn";
import type { JSX, KeyboardEvent } from "react";

import { type ToolTab, useUi } from "@/stores/ui";

const TABS: ReadonlyArray<{ id: ToolTab; label: string }> = [
  { id: "analyze", label: "Analyze" },
  { id: "master", label: "Master" },
  { id: "stems", label: "Stems" },
  { id: "controller", label: "Controller" },
];

/**
 * Right-hand tool panel with the four tabs. Foundation version: the tab bar is
 * complete (roving tabindex, arrow keys); the panels are filled in PR C.
 */
export function ToolPanel(): JSX.Element {
  const active = useUi((s) => s.activeTool);
  const setActive = useUi((s) => s.setActiveTool);

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>): void => {
    const index = TABS.findIndex((t) => t.id === active);
    let nextIndex: number;
    if (event.key === "ArrowRight") nextIndex = (index + 1) % TABS.length;
    else if (event.key === "ArrowLeft")
      nextIndex = (index - 1 + TABS.length) % TABS.length;
    else if (event.key === "Home") nextIndex = 0;
    else if (event.key === "End") nextIndex = TABS.length - 1;
    else return;
    event.preventDefault();
    const next = TABS[nextIndex];
    if (next) {
      setActive(next.id);
      document.getElementById(`tool-tab-${next.id}`)?.focus();
    }
  };

  return (
    <aside aria-label="Tools" className="flex h-full flex-col border-l border-border">
      <div
        role="tablist"
        aria-label="Tools"
        className="flex gap-0.5 border-b border-border px-3"
      >
        {TABS.map((t) => (
          <button
            key={t.id}
            id={`tool-tab-${t.id}`}
            type="button"
            role="tab"
            aria-selected={active === t.id}
            aria-controls="tool-panel"
            tabIndex={active === t.id ? 0 : -1}
            onClick={() => setActive(t.id)}
            onKeyDown={onKeyDown}
            className={cn(
              "border-b-2 border-transparent px-3.5 py-2.5 font-semibold text-muted-foreground",
              active === t.id && "border-primary text-foreground",
            )}
          >
            {t.label}
          </button>
        ))}
      </div>
      <div
        role="tabpanel"
        id="tool-panel"
        tabIndex={0}
        aria-labelledby={`tool-tab-${active}`}
        className="grid flex-1 place-items-center p-4 text-muted-foreground"
      >
        {TABS.find((t) => t.id === active)?.label} tools arrive in the next step.
      </div>
    </aside>
  );
}
