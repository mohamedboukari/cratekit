/** VirtualDJ export, model download and the file drop overlay. */
export const dialogs = {
  export: {
    title: "Export to VirtualDJ",
    nothingToChange: "Nothing to change: VirtualDJ already has this data.",
    willWrite_one:
      "These changes will be written for {{count}} track. VirtualDJ's database is backed up first.",
    willWrite_other:
      "These changes will be written for {{count}} tracks. VirtualDJ's database is backed up first.",
    prepareFailed: "The export could not be prepared.",
    checking: "Checking what would change...",
    backup: "Backup:",
    backupPath: "Backup: {{path}}",
    cancel: "Cancel",
    writing: "Writing...",
    confirm: "Export",
    exported_one: "Exported {{count}} track to VirtualDJ",
    exported_other: "Exported {{count}} tracks to VirtualDJ",
    /** One line per value written (the UI words Rust's data). */
    changes: {
      bpm: "BPM {{value}}",
      key: "Key {{value}}",
      cues_one: "{{count}} hot cue",
      cues_other: "{{count}} hot cues",
      unknown: "?",
    },
  },
  modelDownload: {
    title: "Download the stems model",
    sizeUnknown: "about 200 MB",
    size: "{{mb}} MB",
    description:
      "Stem splitting uses the htdemucs model ({{size}}). It downloads once, its checksum is verified, and it stays on this computer. This is the only time Cratekit goes online.",
    cancel: "Cancel download",
    hide: "Hide",
    notNow: "Not now",
    download: "Download {{size}}",
  },
  dropOverlay: {
    title: "Drop tracks or a folder to add them",
    hint: "Your files stay where they are",
  },
  errors: {
    exportPrepare: "Could not prepare the VirtualDJ export",
    exportWrite: "The VirtualDJ export failed; your database was not changed",
  },
} as const;
