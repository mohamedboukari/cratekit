import type { Translation } from "@/i18n/en";

/** Titres d'erreur des stores et de lib, et traductions des codes d'erreur Rust. */
export const errors: Translation["errors"] = {
  notBuilt: {
    title: "Pas encore disponible",
    detail: "Cette partie de Cratekit arrive dans une prochaine version.",
  },
  codes: {
    io: "Un fichier n'a pas pu être lu ou écrit.",
    not_found: "Cet élément n'existe plus.",
    model_missing: "Le modèle de stems n'est pas encore téléchargé.",
    window: "Une fenêtre n'a pas pu s'ouvrir.",
    unsupported_format: "Ce fichier n'est pas un format audio que Cratekit sait lire.",
    decode: "Ce fichier audio n'a pas pu être décodé.",
    no_audio: "Ce fichier ne contient pas d'audio.",
    audio_processing: "L'audio n'a pas pu être traité.",
    database: "La base de la bibliothèque n'a pas pu être lue ou écrite.",
    database_too_new:
      "Cette bibliothèque vient d'une version plus récente de Cratekit. Mettez l'app à jour pour l'ouvrir.",
    no_audio_device: "Aucune sortie audio trouvée. Branchez des enceintes ou un casque.",
    audio_device: "La sortie audio n'a pas pu être utilisée.",
    invalid_setting: "Ce réglage ne peut pas prendre cette valeur.",
    invalid_name: "Le nom ne peut pas être vide.",
    download:
      "Le téléchargement n'a pas abouti. Vérifiez la connexion et réessayez : il reprendra là où il s'est arrêté.",
    checksum:
      "Le fichier téléchargé n'était pas celui attendu et a été supprimé. Réessayez.",
    busy: "Attendez la fin du téléchargement.",
    stems: "Les stems n'ont pas pu être séparés.",
  },
  controller: {
    load: "Impossible de charger l'assignation du contrôleur",
    learnStart: "Impossible de lancer l'apprentissage MIDI",
    learnCancel: "Impossible d'annuler l'apprentissage MIDI",
  },
  jobs: {
    failed: "Une tâche en arrière-plan a échoué",
    cancel: "Impossible d'annuler la tâche",
    pause: "Impossible de mettre les tâches en pause",
    resume: "Impossible de reprendre les tâches",
  },
  library: {
    load: "Impossible de charger la bibliothèque",
    refresh: "Impossible d'actualiser la bibliothèque",
    add: "Impossible d'ajouter les morceaux",
  },
  master: {
    presets: "Impossible de charger les presets de mastering",
    levelMatch: "Impossible de modifier l'égalisation du niveau",
    preview: "Impossible de préparer l'aperçu masterisé",
    export: "Impossible d'exporter le master",
  },
  player: {
    refresh: "Impossible d'actualiser le morceau",
    load: "Impossible de charger le morceau",
    playback: "La lecture a échoué",
    seek: "Impossible de se déplacer dans le morceau",
    hearing: "Impossible de changer ce que vous entendez",
    filter: "Impossible de modifier le filtre",
    cue: "Impossible de revenir au point de cue",
    hotCueSet: "Impossible de placer le hot cue {{slot}}",
    hotCueClear: "Impossible d'effacer le hot cue {{slot}}",
    hotCueChange: "Impossible de modifier le hot cue {{slot}}",
  },
  setlist: {
    update: "Impossible de mettre à jour la set list",
    check: "Impossible de vérifier la set list",
    load: "Impossible de charger la set list",
    refresh: "Impossible d'actualiser la set list",
    add: "Impossible d'ajouter à la set list",
    saveCrate: "Impossible d'enregistrer le set comme bac",
  },
  settings: {
    load: "Impossible de charger les réglages, réglages par défaut utilisés",
    save: "Impossible d'enregistrer le réglage",
    audioOutput: "Impossible de changer de sortie audio",
    folderPicker: "Impossible d'ouvrir le sélecteur de dossier",
    readModels: "Impossible de lire les modèles",
    download: "Impossible de lancer le téléchargement",
    deleteModel: "Impossible de supprimer le modèle",
  },
  stems: {
    read: "Impossible de lire les stems",
    split: "Impossible de séparer les stems",
    downloadModel: "Impossible de télécharger le modèle de stems",
    volume: "Impossible de modifier le volume du stem",
    mute: "Impossible de couper le stem",
    solo: "Impossible de mettre le stem en solo",
    export: "Impossible d'exporter les stems",
  },
  ui: {
    openSettings: "Impossible d'ouvrir les réglages",
  },
};
