import type { Translation } from "@/i18n/en";

/** Le deck : en-tête, stats, forme d'onde, transport, éditeur de hot cue. */
export const deck: Translation["deck"] = {
  label: "Deck",
  empty:
    "Double-cliquez sur un morceau (ou appuyez sur Entrée) pour le charger sur le deck",
  stats: {
    key: "Tonalité",
    energy: "Énergie",
    confidence: "confiance {{percent}} %",
  },
  waveform: {
    notAnalyzed: "Pas encore analysé : la forme d'onde apparaît après l'analyse",
    position: "Position dans le morceau",
    positionValue: "{{time}} sur {{duration}}",
    hotCues: "Hot cues",
    flag: "Hot cue {{slot}} à {{time}}. Delete pour effacer, Shift+Enter pour modifier",
    flagNamed:
      "Hot cue {{slot}} à {{time}}, {{name}}. Delete pour effacer, Shift+Enter pour modifier",
  },
  transport: {
    cueTitle: "Cue (C)",
    play: "Lecture",
    pause: "Pause",
    playTitle: "Lecture / pause (Space)",
    padsLegend: "Hot cues {{from}} à {{to}}",
    padSet: "Hot cue {{slot}}, aller à {{time}}",
    padEmpty: "Hot cue {{slot}}, vide : placer à la tête de lecture",
    padTitle: "Hot cue {{slot}} ({{slot}}). Shift+clic pour effacer",
    showPads: "Afficher les hot cues {{from}} à {{to}}",
    hearingLabel: "Écoute :",
    hearing: {
      original: "Original",
      mastered: "Masterisé",
      stems: "Mix des stems",
    },
    filter: "Filtre",
  },
  knob: {
    title: "{{label}} : {{value}}",
  },
  cueEditor: {
    title: "Hot cue {{slot}}",
    name: "Nom",
    namePlaceholder: "ex. Drop, Entrée voix",
    colour: "Couleur",
    colors: {
      orange: "orange",
      blue: "bleu",
      pink: "rose",
      purple: "violet",
      lime: "vert citron",
      yellow: "jaune",
      red: "rouge",
      teal: "bleu canard",
    },
    delete: "Supprimer le cue",
    save: "Enregistrer",
  },
};
