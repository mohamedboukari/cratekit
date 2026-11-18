import type { JSX } from "react";
import { useTranslation } from "react-i18next";

import { SettingsView } from "@/components/settings/SettingsView";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog";
import { useUi } from "@/stores/ui";

/**
 * Settings as an in-page window, for the browser demo (a browser cannot open
 * a second native window). The desktop app opens a real window instead.
 */
export function SettingsDialog(): JSX.Element {
  const { t } = useTranslation();
  const open = useUi((s) => s.settingsOpen);
  const close = useUi((s) => s.closeSettings);
  return (
    <Dialog open={open} onOpenChange={(next) => !next && close()}>
      <DialogContent className="h-[560px] max-h-[calc(100%-2rem)] grid-rows-[auto_1fr] gap-0 overflow-hidden p-0 sm:max-w-[760px]">
        <DialogTitle className="border-b border-border px-4 py-2.5">
          {t("settings.title")}
        </DialogTitle>
        <div className="min-h-0 flex-1">
          <SettingsView />
        </div>
      </DialogContent>
    </Dialog>
  );
}
