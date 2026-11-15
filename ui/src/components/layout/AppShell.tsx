import type { JSX, ReactNode } from "react";

import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { usePaneLayout } from "@/hooks/usePaneLayout";

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
 * user's resized layout is saved through settings (usePaneLayout) and restored.
 * The tool panel collapses before the track browser gets too small.
 * Minimum widths add up to fit the 1024 px minimum window: sidebar 170 +
 * browser 550 (tracks 360 + set list 190) + tools 280, plus the handles.
 */
export function AppShell({
  sidebar,
  deck,
  browser,
  tools,
  statusBar,
  banner,
}: AppShellProps): JSX.Element {
  const root = usePaneLayout("root");
  const main = usePaneLayout("main");
  const work = usePaneLayout("work");

  return (
    <div className="flex h-full flex-col">
      {banner}
      <ResizablePanelGroup
        id="root"
        orientation="horizontal"
        className="min-h-0 flex-1"
        {...root}
      >
        <ResizablePanel
          id="sidebar"
          defaultSize={210}
          minSize={170}
          maxSize={320}
          collapsible
          collapsedSize={0}
        >
          {sidebar}
        </ResizablePanel>
        <ResizableHandle />
        <ResizablePanel id="main" minSize={832}>
          <ResizablePanelGroup id="main" orientation="vertical" {...main}>
            <ResizablePanel id="deck" defaultSize={224} minSize={220} maxSize={340}>
              {deck}
            </ResizablePanel>
            <ResizableHandle />
            <ResizablePanel id="work" minSize={240}>
              <ResizablePanelGroup id="work" orientation="horizontal" {...work}>
                <ResizablePanel id="browser" minSize={550}>
                  {browser}
                </ResizablePanel>
                <ResizableHandle />
                <ResizablePanel
                  id="tools"
                  defaultSize={340}
                  minSize={280}
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
