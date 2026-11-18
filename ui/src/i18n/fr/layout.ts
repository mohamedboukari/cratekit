import type { Translation } from "@/i18n/en";

/** Fenêtre, barre latérale, barre d'état, bandeau de démo et chargement. */
export const layout: Translation["layout"] = {
  app: {
    loading: "Chargement de Cratekit...",
  },
  demoBanner:
    "Démo avec des données fictives. L'app de bureau analyse vos propres morceaux, hors ligne.",
  sidebar: {
    label: "Bibliothèque",
    library: "Bibliothèque",
    allTracks: "Tous les morceaux",
    needsAnalysis: "À analyser",
    crates: "Bacs",
    devices: "Appareils",
    connected: "connecté",
    noController: "Branchez un contrôleur ou utilisez les raccourcis clavier",
    settings: "Réglages",
    settingsShortcut: "Réglages ({{shortcut}})",
  },
  statusBar: {
    noController: "Aucun contrôleur",
    jobsLabel: "Tâches en arrière-plan : {{running}}. Afficher la file",
    noJobsLabel: "Aucune tâche en arrière-plan. Afficher la file",
    noJobs: "Aucune tâche en arrière-plan",
    more_one: "+{{count}} autre",
    more_other: "+{{count}} autres",
    paused: "en pause",
    jobs: "Tâches en arrière-plan",
    resume: "Reprendre",
    pause: "Pause",
    nothingRunning:
      "Rien en cours. L'analyse, les stems et les exports apparaissent ici.",
    cancelJob: "Annuler {{label}}",
    backendChecking: "Moteur : vérification",
    offline: "Hors ligne · rien n'est envoyé",
  },
};
