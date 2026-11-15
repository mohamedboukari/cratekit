import type { JSX, ReactNode } from "react";
import type { Layout, LayoutChangedMeta } from "react-resizable-panels";

import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { useSettings } from "@/stores/settings";

/** Stable fallback so the selector does not return a new object every render. */
const NO_LAYOUTS: Record<string, Record<string, number>> = {};

interface AppShellProps {
  sidebar: ReactNode;
  deck: ReactNode;
  browser: ReactNode;
  tools: ReactNode;
  statusBar: ReactNode;
  banner?: ReactNode;
}

/**
 * Laptop-first window layout (docs/UI.md): sidebar | [deck / (browser | tools)],
 * status bar at the bottom. Sizes in pixels set the defaults and limits; the
 * user's resized layout (percentages) is saved through settings and restored.
 * The tool panel collapses before the track browser gets too small.
 */
export function AppShell({
  sidebar,
  deck,
  browser,
  tools,
  statusBar,
  banner,
}: AppShellProps): JSX.Element {
  const layouts = useSettings((s) => s.settings?.paneLayouts ?? NO_LAYOUTS);
  const setPaneLayout = useSettings((s) => s.setPaneLayout);

  // onLayoutChanged also fires on mount and when the window resizes. Save only
  // real drags, and save the requested layout (before pixel limits clamped it),
  // so a session in a small window does not overwrite the user's sizes.
  const save =
    (groupId: string) =>
    (layout: Layout, meta: LayoutChangedMeta): void => {
      if (!meta.isUserInteraction) return;
      void setPaneLayout(groupId, meta.requestedLayout ?? layout);
    };

  return (
    <div className="flex h-full flex-col">
      {banner}
      <ResizablePanelGroup
        id="root"
        orientation="horizontal"
        className="min-h-0 flex-1"
        defaultLayout={layouts["root"]}
        onLayoutChanged={save("root")}
      >
        <ResizablePanel
          id="sidebar"
          defaultSize={210}
          minSize={180}
          maxSize={320}
          collapsible
          collapsedSize={0}
        >
          {sidebar}
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="main" minSize={640}>
          <ResizablePanelGroup
            id="main"
            orientation="vertical"
            defaultLayout={layouts["main"]}
            onLayoutChanged={save("main")}
          >
            <ResizablePanel id="deck" defaultSize={210} minSize={170} maxSize={340}>
              {deck}
            </ResizablePanel>
            <ResizableHandle />
            <ResizablePanel id="work" minSize={240}>
              <ResizablePanelGroup
                id="work"
                orientation="horizontal"
                defaultLayout={layouts["work"]}
                onLayoutChanged={save("work")}
              >
                <ResizablePanel id="browser" minSize={360}>
                  {browser}
                </ResizablePanel>
                <ResizableHandle />
                <ResizablePanel
                  id="tools"
                  defaultSize={380}
                  minSize={300}
                  maxSize={560}
                  collapsible
                  collapsedSize={0}
                >
                  {tools}
                </ResizablePanel>
              </ResizablePanelGroup>
            </ResizablePanel>
          </ResizablePanelGroup>
        </ResizablePanel>
      </ResizablePanelGroup>
      {statusBar}
    </div>
  );
}
