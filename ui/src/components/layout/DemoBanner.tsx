import type { JSX } from "react";
import { useTranslation } from "react-i18next";

/** Shown only in mock mode (browser dev server and the GitHub Pages demo). */
export function DemoBanner(): JSX.Element {
  const { t } = useTranslation();
  return (
    <div
      role="note"
      className="shrink-0 bg-primary px-3 py-1 text-center text-[11px] font-semibold text-primary-foreground"
    >
      {t("layout.demoBanner")}
    </div>
  );
}
