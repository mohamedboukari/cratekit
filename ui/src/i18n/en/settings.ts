/** The Settings window and the keyboard shortcut list. */
export const settings = {
  title: "Settings",
  windowTitle: "Cratekit settings",
  sectionsLabel: "Settings sections",
  loading: "Loading settings...",
  sections: {
    general: "General",
    audio: "Audio output",
    library: "Library",
    processing: "Processing",
    models: "Models",
    controller: "Controller",
    keyboard: "Keyboard",
  },
  general: {
    description: "English is the default. Choose another language for the whole app.",
    language: "Language",
  },
  audio: {
    description:
      "Where Cratekit plays. If the device is unplugged, playback moves to the system default.",
    device: "Output device",
    noDevices: "No output devices found",
    systemDefault: "System default ({{name}})",
    bufferSize: "Buffer size",
    bufferHint: "Smaller is more responsive; raise it if you hear clicks.",
    samples: "{{frames}} samples",
    sampleRate: "Sample rate",
  },
  library: {
    description:
      "Folders Cratekit watches for new tracks, and where exports go. Your original files are never changed.",
    watched: "Watched folders",
    empty: "No folders yet.",
    stopWatching: "Stop watching {{folder}}",
    addFolder: "Add folder...",
    autoAdd: "Add new files automatically",
    output: "Output folder",
    outputHint:
      "Masters and stems are written here. When it is empty: your Music folder, in Cratekit.",
    notSet: "Not set",
    choose: "Choose...",
  },
  processing: {
    description: "How stem splitting runs on this computer.",
    backend: "Stems backend",
    auto: "Use the GPU when available",
    autoHint: "CoreML on macOS, DirectML on Windows; falls back to the CPU.",
    cpu: "CPU only",
    cpuHint: "Slower, but works on every machine.",
    inUse: "In use now: {{backend}}",
    inUseReason: "In use now: {{backend}} ({{reason}})",
    /** Why the CPU is used (StemsCpuReason from Rust). */
    cpuReasons: {
      no_gpu: "no supported GPU found",
      cpu_chosen: "CPU only is chosen below",
      gpu_check_failed: "the GPU check failed, so the CPU is used",
    },
    checking: "Checking the backend...",
  },
  models: {
    description:
      "Machine learning models download once, are checked against a checksum, and stay on this computer.",
    checking: "Checking models...",
    empty: "No models to show.",
    retry: "Retry",
    size: "{{value}} MB",
    status: "{{size}}, {{state}}",
    stateDownloading: "downloading {{pct}}%",
    stateDownloaded: "downloaded",
    stateNotDownloaded: "not downloaded",
    download: "Download",
    downloading: "Downloading {{pct}}%",
    deleteForGood: "Delete for good",
    keep: "Keep",
    delete: "Delete...",
  },
  controller: {
    description:
      "USB MIDI controllers are detected when you plug them in. Map controls in the Controller tab of the main window.",
    empty: "No controller found. Plug one in, or use the keyboard shortcuts.",
    connected: "connected",
    notConnected: "not connected",
  },
  keyboard: {
    description:
      "Shortcuts work in the main window, except while you type in a field or a dialog is open.",
  },
} as const;
