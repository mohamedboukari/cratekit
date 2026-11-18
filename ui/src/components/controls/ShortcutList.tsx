import type { JSX } from "react";
import { useTranslation } from "react-i18next";

import { ToolCard } from "@/components/tools/ToolCard";
import { formatShortcut, isMacPlatform, SHORTCUTS } from "@/lib/shortcuts";

/** The keyboard fallback, from the single shortcut map. */
export function ShortcutList({ bare = false }: { bare?: boolean }): JSX.Element {
  const { t } = useTranslation();
  const mac = isMacPlatform();
  const list = (
    <dl className="grid grid-cols-[1fr_auto] gap-x-3 gap-y-1">
      {SHORTCUTS.map((s) => (
        <div key={s.id} className="contents">
          <dt className="text-muted-foreground">
            {s.labelKey === "shortcuts.labels.hotCue"
              ? t(s.labelKey, { n: s.n })
              : t(s.labelKey)}
          </dt>
          <dd>
            <kbd className="rounded-sm border border-border bg-secondary px-1.5 font-mono text-[11px]">
              {formatShortcut(s, mac)}
            </kbd>
          </dd>
        </div>
      ))}
    </dl>
  );
  // Bare: the caller provides the heading (Settings > Keyboard).
  return bare ? list : <ToolCard title={t("shortcuts.title")}>{list}</ToolCard>;
}
