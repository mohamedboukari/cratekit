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

/**
 * The supported formats as a list in `language`: "MP3, WAV, FLAC, AIFF,
 * M4A/AAC, and OGG" in English, "... M4A/AAC et OGG" in French (the
 * browser's own list formatting, so no translated "and" is needed).
 */
export function audioFormatsText(language: string): string {
  return new Intl.ListFormat(language, { type: "conjunction" }).format(
    AUDIO_FORMATS.map((f) => f.label),
  );
}
