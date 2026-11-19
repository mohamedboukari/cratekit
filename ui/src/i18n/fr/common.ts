import type { Translation } from "@/i18n/en";

/** Mots partagés, libellés du menu natif et messages d'import. */
export const common: Translation["common"] = {
  somethingWentWrong: "Une erreur est survenue",
  close: "Fermer",
  notifications: "Notifications",
  menu: {
    settings: "Réglages...",
    settingsWindowTitle: "Réglages de Cratekit",
    file: "Fichier",
    edit: "Édition",
    window: "Fenêtre",
    undo: "Annuler",
    redo: "Rétablir",
    cut: "Couper",
    copy: "Copier",
    paste: "Coller",
    selectAll: "Tout sélectionner",
    minimize: "Réduire",
    closeWindow: "Fermer la fenêtre",
    quit: "Quitter",
    about: "À propos de Cratekit",
    services: "Services",
    hide: "Masquer Cratekit",
    hideOthers: "Masquer les autres",
    showAll: "Tout afficher",
  },
  import: {
    added_one: "{{count}} morceau ajouté",
    added_other: "{{count}} morceaux ajoutés",
    none: "Aucun nouveau fichier audio trouvé",
  },
  format: {
    durationHours: "{{h}} h {{m}} min",
    durationMinutes: "{{m}} min",
    filterOff: "Neutre",
    lowPass: "Passe-bas {{pct}} %",
    highPass: "Passe-haut {{pct}} %",
    backend: {
      coreml: "GPU : CoreML",
      directml: "GPU : DirectML",
      cpu: "CPU",
    },
  },
  jobs: {
    started: "Tâche démarrée : {{label}}",
    done: "Tâche terminée : {{label}}",
    kinds: {
      analysis_one: "Analyse de {{count}} morceau",
      analysis_other: "Analyse de {{count}} morceaux",
      stems: "Séparation des stems",
      master: "Mastering",
      export: "Export",
      modelDownload: "Téléchargement du modèle de stems",
      modelVerify: "Vérification du modèle de stems",
    },
  },
  events: {
    modelReady: "Modèle de stems prêt",
    deviceUnplugged: "Appareil audio déconnecté",
    deviceUnpluggedDetail: "La lecture est passée sur la sortie par défaut du système.",
    deviceLost: "Aucune sortie audio",
    deviceLostDetail:
      "La sortie audio s'est arrêtée et aucune autre n'a pu s'ouvrir. Vérifiez vos enceintes ou votre casque.",
  },
};
