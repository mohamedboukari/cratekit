import { type RefObject, useEffect, useRef } from "react";

import { api } from "@/lib/ipc";
import type { MeterReading } from "@/lib/types";

/**
 * The playhead position (seconds), updated about 30 times a second.
 * Lives in a ref on purpose: canvas components read it in a
 * requestAnimationFrame loop, so React never re-renders at 30 Hz.
 */
export function usePlaybackPositionRef(): RefObject<number> {
  const position = useRef(0);
  useEffect(() => {
    const sub = api().onPlaybackPosition((sec) => {
      position.current = sec;
    });
    return () => {
      void sub.then((stop) => stop());
    };
  }, []);
  return position;
}

const SILENCE: MeterReading = { peakDb: -96, rmsDb: -96 };

/** The latest output meter reading, updated about 30 times a second (same rule as above). */
export function useMeterRef(): RefObject<MeterReading> {
  const meter = useRef<MeterReading>(SILENCE);
  useEffect(() => {
    const sub = api().onMeter((reading) => {
      meter.current = reading;
    });
    return () => {
      void sub.then((stop) => stop());
    };
  }, []);
  return meter;
}
