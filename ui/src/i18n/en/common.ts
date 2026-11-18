/** Shared words, the native menu labels and import messages. */
export const common = {
  somethingWentWrong: "Something went wrong",
  close: "Close",
  notifications: "Notifications",
  menu: {
    settings: "Settings...",
    settingsWindowTitle: "Cratekit Settings",
    file: "File",
    edit: "Edit",
    window: "Window",
    undo: "Undo",
    redo: "Redo",
    cut: "Cut",
    copy: "Copy",
    paste: "Paste",
    selectAll: "Select All",
    minimize: "Minimize",
    closeWindow: "Close Window",
    quit: "Quit",
    about: "About Cratekit",
    services: "Services",
    hide: "Hide Cratekit",
    hideOthers: "Hide Others",
    showAll: "Show All",
  },
  import: {
    added_one: "Added {{count}} track",
    added_other: "Added {{count}} tracks",
    none: "No new audio files found",
  },
  /** Display formatting (lib/format.ts). */
  format: {
    durationHours: "{{h}} h {{m}} min",
    durationMinutes: "{{m}} min",
    filterOff: "Off",
    lowPass: "Low-pass {{pct}}%",
    highPass: "High-pass {{pct}}%",
    backend: {
      coreml: "GPU: CoreML",
      directml: "GPU: DirectML",
      cpu: "CPU",
    },
  },
  /** Screen reader announcements for background jobs (stores/jobs.ts). */
  jobs: {
    started: "{{label}} started",
    done: "{{label}} done",
    /** What a background job is doing, built from its kind (and stage), never from Rust's text. */
    kinds: {
      analysis_one: "Analyzing {{count}} track",
      analysis_other: "Analyzing {{count}} tracks",
      stems: "Splitting stems",
      master: "Mastering",
      export: "Exporting",
      modelDownload: "Downloading the stems model",
      modelVerify: "Verifying the stems model checksum",
    },
  },
  /** Toasts for Rust events (hooks/useIpcEvents.ts). */
  events: {
    modelReady: "Stems model ready",
    deviceUnplugged: "Audio device disconnected",
    deviceUnpluggedDetail: "Playback switched to the system default output.",
  },
} as const;
