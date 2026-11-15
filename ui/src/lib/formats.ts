/**
 * Audio formats Cratekit imports (SPEC, Module 1). Used for the file picker
 * filter and the first-run copy; Rust still checks every file it is given.
 */
export const AUDIO_FORMATS = [
  { label: "MP3", extensions: ["mp3"] },
  { label: "WAV", extensions: ["wav"] },
  { label: "FLAC", extensions: ["flac"] },
  { label: "AIFF", extensions: ["aiff", "aif"] },
  { label: "M4A/AAC", extensions: ["m4a", "aac"] },
  { label: "OGG", extensions: ["ogg"] },
] as const;

export const AUDIO_EXTENSIONS: string[] = AUDIO_FORMATS.flatMap((f) => [...f.extensions]);

/** "MP3, WAV, FLAC, AIFF, M4A/AAC and OGG". */
export const AUDIO_FORMATS_TEXT = `${AUDIO_FORMATS.slice(0, -1)
  .map((f) => f.label)
  .join(", ")} and ${AUDIO_FORMATS.at(-1)?.label ?? ""}`;
