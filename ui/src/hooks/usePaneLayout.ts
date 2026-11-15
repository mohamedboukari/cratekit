import type { Layout, LayoutChangedMeta } from "react-resizable-panels";

import { useSettings } from "@/stores/settings";

interface PaneLayoutProps {
  defaultLayout: Layout | undefined;
  onLayoutChanged: (layout: Layout, meta: LayoutChangedMeta) => void;
}

/**
 * Saved layout for one resizable panel group, restored on mount.
 * onLayoutChanged also fires on mount and when the window resizes: only real
 * drags are saved, and the requested (unclamped) layout is stored, so a
 * session in a small window does not overwrite the user's sizes.
 */
export function usePaneLayout(groupId: string): PaneLayoutProps {
  const defaultLayout = useSettings((s) => s.settings?.paneLayouts[groupId]);
  const setPaneLayout = useSettings((s) => s.setPaneLayout);
  return {
    defaultLayout,
    onLayoutChanged: (layout, meta) => {
      if (!meta.isUserInteraction) return;
      void setPaneLayout(groupId, meta.requestedLayout ?? layout);
    },
  };
}
