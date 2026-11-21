import { useEffect, useRef, type JSX } from "react";

import { onDragMove } from "@/lib/drag";
import { useDrag } from "@/stores/drag";

/** Offset of the label from the pointer, so it never covers the drop line. */
const OFFSET_X = 14;
const OFFSET_Y = 10;

/**
 * The label that follows the pointer during an in-app drag ("3 tracks", or
 * the title). Always mounted, so the first move positions it before it shows;
 * moves update its transform directly, not through React renders.
 */
export function DragGhost(): JSX.Element {
  const payload = useDrag((s) => s.payload);
  const ref = useRef<HTMLDivElement>(null);

  useEffect(
    () =>
      onDragMove((x, y) => {
        if (ref.current)
          ref.current.style.transform = `translate(${x + OFFSET_X}px, ${y + OFFSET_Y}px)`;
      }),
    [],
  );

  return (
    <div
      ref={ref}
      aria-hidden="true"
      hidden={payload === null}
      className="pointer-events-none fixed top-0 left-0 z-50 max-w-64 truncate rounded-md border border-border bg-card px-2 py-1 text-[12px] shadow-lg"
    >
      {payload?.label}
    </div>
  );
}
