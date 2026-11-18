import { cn } from "cn";
import type { JSX, KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import { AnalyzePanel } from "@/components/tools/AnalyzePanel";
import { ControllerPanel } from "@/components/tools/ControllerPanel";
import { MasterPanel } from "@/components/tools/MasterPanel";
import { StemsPanel } from "@/components/tools/StemsPanel";
import { type ToolTab, useUi } from "@/stores/ui";

const PANELS: Record<ToolTab, () => JSX.Element> = {
  analyze: AnalyzePanel,
  master: MasterPanel,
  stems: StemsPanel,
  controller: ControllerPanel,
};

const TABS = [
  { id: "analyze", label: "tools.panel.tabs.analyze" },
  { id: "master", label: "tools.panel.tabs.master" },
  { id: "stems", label: "tools.panel.tabs.stems" },
  { id: "controller", label: "tools.panel.tabs.controller" },
] as const satisfies ReadonlyArray<{ id: ToolTab; label: string }>;

/**
 * Right-hand tool panel with the four tabs (roving tabindex, arrow keys).
 * Switching tabs also switches what you hear (hooks/useHearing.ts).
 */
export function ToolPanel(): JSX.Element {
  const { t } = useTranslation();
  const active = useUi((s) => s.activeTool);
  const setActive = useUi((s) => s.setActiveTool);
  const Panel = PANELS[active];

  const onKeyDown = (event: KeyboardEvent<HTMLButtonElement>): void => {
    const index = TABS.findIndex((tab) => tab.id === active);
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
    <aside
      aria-label={t("tools.panel.label")}
      className="flex h-full flex-col border-l border-border"
    >
      <div
        role="tablist"
        aria-label={t("tools.panel.label")}
        className="flex gap-0.5 border-b border-border px-1"
      >
        {TABS.map((tab) => (
          <button
            key={tab.id}
            id={`tool-tab-${tab.id}`}
            type="button"
            role="tab"
            aria-selected={active === tab.id}
            aria-controls="tool-panel"
            tabIndex={active === tab.id ? 0 : -1}
            onClick={() => setActive(tab.id)}
            onKeyDown={onKeyDown}
            className={cn(
              "min-w-0 flex-1 truncate border-b-2 border-transparent px-1.5 py-2.5 text-center font-semibold text-muted-foreground",
              active === tab.id && "border-primary text-foreground",
            )}
          >
            {t(tab.label)}
          </button>
        ))}
      </div>
      <div
        role="tabpanel"
        id="tool-panel"
        tabIndex={0}
        aria-labelledby={`tool-tab-${active}`}
        className="min-h-0 flex-1 overflow-y-auto -outline-offset-2"
      >
        <Panel />
      </div>
    </aside>
  );
}
