import { FolderPlus } from "lucide-react";
import { useEffect, useState } from "react";
import type { JSX } from "react";

import { Button } from "@/components/ui/button";
import { appVersion } from "@/lib/ipc";

/** App shell placeholder until the layout task: empty state + status bar. */
export function App(): JSX.Element {
  const [version, setVersion] = useState<string | null>(null);

  useEffect(() => {
    appVersion()
      .then(setVersion)
      .catch(() => setVersion(null));
  }, []);

  return (
    <div className="flex h-full flex-col">
      <main className="flex flex-1 flex-col items-center justify-center gap-4">
        <h1 className="font-heading text-2xl font-bold">Cratekit</h1>
        <div className="flex flex-col items-center gap-3 rounded-xl border border-dashed border-ck-line px-10 py-8 text-muted-foreground">
          <span>Drop tracks or a folder</span>
          {/* Wired up in the library task (library_add_paths). */}
          <Button size="lg" disabled>
            <FolderPlus data-icon="inline-start" aria-hidden="true" />
            Add tracks
          </Button>
        </div>
      </main>
      <footer className="flex h-7 items-center gap-4 border-t border-ck-line bg-sidebar px-3.5 text-[11px] text-muted-foreground">
        <span>Offline · nothing uploaded</span>
        <span className="ml-auto font-mono">{version ? `v${version}` : "v?"}</span>
      </footer>
    </div>
  );
}
