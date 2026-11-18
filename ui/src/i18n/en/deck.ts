/** The deck: header, stats, waveform, transport, hot cue editor. */
export const deck = {
  label: "Deck",
  empty: "Double-click a track (or press Enter) to load it on the deck",
  stats: {
    key: "Key",
    energy: "Energy",
    confidence: "confidence {{percent}}%",
  },
  waveform: {
    notAnalyzed: "Not analyzed yet: the waveform appears after analysis",
    position: "Track position",
    positionValue: "{{time}} of {{duration}}",
    hotCues: "Hot cues",
    flag: "Hot cue {{slot}} at {{time}}. Delete to clear, Shift+Enter to edit",
    flagNamed:
      "Hot cue {{slot}} at {{time}}, {{name}}. Delete to clear, Shift+Enter to edit",
  },
  transport: {
    cueTitle: "Cue (C)",
    play: "Play",
    pause: "Pause",
    playTitle: "Play / pause (Space)",
    padsLegend: "Hot cues {{from}} to {{to}}",
    padSet: "Hot cue {{slot}}, jump to {{time}}",
    padEmpty: "Hot cue {{slot}}, empty: set at playhead",
    padTitle: "Hot cue {{slot}} ({{slot}}). Shift-click to clear",
    showPads: "Show hot cues {{from}} to {{to}}",
    hearingLabel: "Hearing:",
    hearing: {
      original: "Original",
      mastered: "Mastered",
      stems: "Stems mix",
    },
    filter: "Filter",
  },
  knob: {
    title: "{{label}}: {{value}}",
  },
  cueEditor: {
    title: "Hot cue {{slot}}",
    name: "Name",
    namePlaceholder: "e.g. Drop, Vocal in",
    colour: "Colour",
    colors: {
      orange: "orange",
      blue: "blue",
      pink: "pink",
      purple: "purple",
      lime: "lime",
      yellow: "yellow",
      red: "red",
      teal: "teal",
    },
    delete: "Delete cue",
    save: "Save",
  },
} as const;
