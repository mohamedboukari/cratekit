import type { Translation } from "@/i18n/en";

/** La fenêtre Réglages et la liste des raccourcis clavier. */
export const settings: Translation["settings"] = {
  title: "Réglages",
  windowTitle: "Réglages de Cratekit",
  sectionsLabel: "Sections des réglages",
  loading: "Chargement des réglages...",
  sections: {
    general: "Général",
    audio: "Sortie audio",
    library: "Bibliothèque",
    processing: "Traitement",
    models: "Modèles",
    controller: "Contrôleur",
    keyboard: "Clavier",
  },
  general: {
    description:
      "L'anglais est la langue par défaut. Choisissez une autre langue pour toute l'application.",
    language: "Langue",
  },
  audio: {
    description:
      "La sortie où Cratekit joue le son. Si l'appareil est débranché, la lecture passe sur la sortie par défaut du système.",
    device: "Appareil de sortie",
    noDevices: "Aucun appareil de sortie trouvé",
    systemDefault: "Défaut du système ({{name}})",
    bufferSize: "Taille du tampon",
    bufferHint: "Plus petit, plus réactif. Augmentez-la si vous entendez des clics.",
    samples: "{{frames}} échantillons",
    sampleRate: "Fréquence d'échantillonnage",
  },
  library: {
    description:
      "Les dossiers que Cratekit surveille pour trouver de nouveaux morceaux, et l'emplacement des exports. Vos fichiers d'origine ne sont jamais modifiés.",
    watched: "Dossiers surveillés",
    empty: "Aucun dossier pour l'instant.",
    stopWatching: "Ne plus surveiller {{folder}}",
    addFolder: "Ajouter un dossier...",
    autoAdd: "Ajouter les nouveaux fichiers automatiquement",
    output: "Dossier de sortie",
    outputHint:
      "Les masters et les stems sont enregistrés ici. S'il est vide : votre dossier Musique, dans Cratekit.",
    notSet: "Non défini",
    choose: "Choisir...",
  },
  processing: {
    description: "Comment la séparation des stems s'exécute sur cet ordinateur.",
    backend: "Moteur des stems",
    auto: "Utiliser le GPU si disponible",
    autoHint: "CoreML sur macOS, DirectML sur Windows. Sinon, le CPU.",
    cpu: "CPU uniquement",
    cpuHint: "Plus lent, mais fonctionne sur toutes les machines.",
    inUse: "Utilisé actuellement : {{backend}}",
    inUseReason: "Utilisé actuellement : {{backend}} ({{reason}})",
    cpuReasons: {
      no_gpu: "aucun GPU compatible trouvé",
      cpu_chosen: "le mode CPU seul est choisi ci-dessous",
      gpu_check_failed: "la vérification du GPU a échoué, le CPU est utilisé",
    },
    checking: "Vérification du moteur...",
  },
  models: {
    description:
      "Les modèles d'apprentissage automatique se téléchargent une seule fois, sont vérifiés par somme de contrôle et restent sur cet ordinateur.",
    checking: "Vérification des modèles...",
    empty: "Aucun modèle à afficher.",
    retry: "Réessayer",
    size: "{{value}} Mo",
    status: "{{size}}, {{state}}",
    stateDownloading: "téléchargement {{pct}} %",
    stateDownloaded: "téléchargé",
    stateNotDownloaded: "non téléchargé",
    download: "Télécharger",
    downloading: "Téléchargement {{pct}} %",
    deleteForGood: "Supprimer définitivement",
    keep: "Garder",
    delete: "Supprimer...",
  },
  controller: {
    description:
      "Les contrôleurs MIDI USB sont détectés dès que vous les branchez. Assignez les commandes dans l'onglet Contrôleur de la fenêtre principale.",
    empty: "Aucun contrôleur trouvé. Branchez-en un, ou utilisez les raccourcis clavier.",
    connected: "connecté",
    notConnected: "non connecté",
  },
  keyboard: {
    description:
      "Les raccourcis fonctionnent dans la fenêtre principale, sauf pendant la saisie dans un champ ou quand une boîte de dialogue est ouverte.",
  },
};
