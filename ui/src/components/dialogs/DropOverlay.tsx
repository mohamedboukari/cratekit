import { FolderInput } from "lucide-react";
import { useEffect, useState, type JSX } from "react";

import { api } from "@/lib/ipc";
import { announceImport } from "@/lib/notify";
import { useLibrary } from "@/stores/library";

/**
 * Drag files or folders from the system onto the window to add them: shows a
 * full-window drop target while dragging, then imports on drop. Rust reads
 * and checks every file.
 */
export function DropOverlay(): JSX.Element | null {
  const [dragging, setDragging] = useState(false);
  const addPaths = useLibrary((s) => s.addPaths);

  useEffect(() => {
    const importDropped = async (paths: string[]): Promise<void> => {
      announceImport(await addPaths(paths));
    };
    const sub = api().onFilesDropped((drop) => {
      if (drop.kind === "enter" || drop.kind === "over") setDragging(true);
      else if (drop.kind === "leave") setDragging(false);
      else {
        setDragging(false);
        void importDropped(drop.paths);
      }
    });
    return () => {
      void sub.then((stop) => stop());
    };
  }, [addPaths]);

  if (!dragging) return null;
  return (
    <div
      aria-hidden="true"
      className="pointer-events-none fixed inset-0 z-50 grid place-items-center bg-background/80 p-6"
    >
      <div className="flex h-full w-full flex-col items-center justify-center gap-3 rounded-2xl border-2 border-dashed border-primary text-primary">
        <FolderInput className="size-10" />
        <p className="text-lg font-semibold">Drop tracks or a folder to add them</p>
        <p className="text-muted-foreground">Your files stay where they are</p>
      </div>
    </div>
  );
}
