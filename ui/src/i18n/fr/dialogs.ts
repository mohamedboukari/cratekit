import type { Translation } from "@/i18n/en";

/** Export VirtualDJ, téléchargement du modèle et zone de dépôt de fichiers. */
export const dialogs: Translation["dialogs"] = {
  export: {
    title: "Exporter vers VirtualDJ",
    nothingToChange: "Rien à changer : VirtualDJ a déjà ces données.",
    willWrite_one:
      "Ces changements seront écrits pour {{count}} morceau. La base de données de VirtualDJ est d'abord sauvegardée.",
    willWrite_other:
      "Ces changements seront écrits pour {{count}} morceaux. La base de données de VirtualDJ est d'abord sauvegardée.",
    prepareFailed: "L'export n'a pas pu être préparé.",
    checking: "Vérification des changements...",
    backup: "Sauvegarde :",
    backupPath: "Sauvegarde : {{path}}",
    cancel: "Annuler",
    writing: "Écriture...",
    confirm: "Exporter",
    exported_one: "{{count}} morceau exporté vers VirtualDJ",
    exported_other: "{{count}} morceaux exportés vers VirtualDJ",
    changes: {
      bpm: "BPM {{value}}",
      key: "Tonalité {{value}}",
      cues_one: "{{count}} hot cue",
      cues_other: "{{count}} hot cues",
      unknown: "?",
    },
  },
  modelDownload: {
    title: "Télécharger le modèle de stems",
    sizeUnknown: "environ 170 Mo",
    size: "{{mb}} Mo",
    description:
      "La séparation des stems utilise le modèle htdemucs ({{size}}), téléchargé depuis Hugging Face (la conversion par Intel de Demucs de Meta). Il est téléchargé une seule fois, sa somme de contrôle est vérifiée, et il reste sur cet ordinateur. C'est la seule fois où Cratekit se connecte à Internet.",
    licence:
      "Le code de Demucs est open source, mais la licence des poids entraînés du modèle n'est pas claire : ils ont été entraînés en partie sur des données réservées à la recherche. Cratekit ne les inclut pas ; téléchargez-les uniquement pour votre usage personnel.",
    cancel: "Annuler le téléchargement",
    hide: "Masquer",
    notNow: "Plus tard",
    download: "Télécharger ({{size}})",
  },
  dropOverlay: {
    title: "Déposez des morceaux ou un dossier pour les ajouter",
    hint: "Vos fichiers restent où ils sont",
  },
  errors: {
    exportPrepare: "Impossible de préparer l'export VirtualDJ",
    exportWrite:
      "L'export VirtualDJ a échoué ; votre base de données n'a pas été modifiée",
  },
};
