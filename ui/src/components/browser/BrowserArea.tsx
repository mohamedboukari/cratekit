import type { JSX } from "react";

import { SetList } from "@/components/browser/SetList";
import { TrackBrowser } from "@/components/browser/TrackBrowser";
import {
  ResizableHandle,
  ResizablePanel,
  ResizablePanelGroup,
} from "@/components/ui/resizable";
import { usePaneLayout } from "@/hooks/usePaneLayout";

/** Track browser with the set builder side list next to it (collapsible). */
export function BrowserArea(): JSX.Element {
  const layout = usePaneLayout("browser");
  return (
    <ResizablePanelGroup id="browser" orientation="horizontal" {...layout}>
      <ResizablePanel id="tracks" minSize={360}>
        <TrackBrowser />
      </ResizablePanel>
      <ResizableHandle />
      <ResizablePanel
        id="setlist"
        defaultSize={260}
        minSize={190}
        maxSize={420}
        collapsible
        collapsedSize={0}
      >
        <SetList />
      </ResizablePanel>
    </ResizablePanelGroup>
  );
}
