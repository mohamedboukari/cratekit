/** Error titles from stores and lib, and translations of Rust error codes. */
export const errors = {
  /** Detail shown for a known `AppError.code`; unknown codes show Rust's message. */
  /** A command the Rust side does not have yet: a later version, not a failure. */
  notBuilt: {
    title: "Not built yet",
    detail: "This part of Cratekit comes in a later version.",
  },
  codes: {
    io: "A file could not be read or written.",
    not_found: "This item no longer exists.",
    model_missing: "The stems model is not downloaded yet.",
    window: "A window could not be opened.",
    unsupported_format: "This file is not an audio format Cratekit can read.",
    decode: "This audio file could not be decoded.",
    no_audio: "This file contains no audio.",
    audio_processing: "The audio could not be processed.",
    database: "The library database could not be read or written.",
    database_too_new:
      "This library was made by a newer Cratekit. Update the app to open it.",
    no_audio_device: "No audio output was found. Connect speakers or headphones.",
    audio_device: "The audio output could not be used.",
    invalid_setting: "This setting cannot take that value.",
    invalid_name: "The name cannot be empty.",
    download:
      "The download did not finish. Check the connection and try again: it continues where it stopped.",
    checksum: "The downloaded file was not the expected one and was deleted. Try again.",
    busy: "Wait until the download finishes.",
    stems: "The stems could not be split.",
  },
  controller: {
    load: "Could not load the controller mapping",
    learnStart: "Could not start MIDI learn",
    learnCancel: "Could not cancel MIDI learn",
  },
  jobs: {
    failed: "A background job failed",
    cancel: "Could not cancel the job",
    pause: "Could not pause jobs",
    resume: "Could not resume jobs",
  },
  library: {
    load: "Could not load the library",
    refresh: "Could not refresh the library",
    add: "Could not add the tracks",
  },
  master: {
    presets: "Could not load the mastering presets",
    levelMatch: "Could not change level matching",
    preview: "Could not prepare the mastered preview",
    export: "Could not export the master",
  },
  player: {
    refresh: "Could not refresh the track",
    load: "Could not load the track",
    playback: "Playback failed",
    seek: "Could not seek",
    hearing: "Could not switch what you hear",
    filter: "Could not change the filter",
    cue: "Could not return to the cue point",
    hotCueSet: "Could not set hot cue {{slot}}",
    hotCueClear: "Could not clear hot cue {{slot}}",
    hotCueChange: "Could not change hot cue {{slot}}",
  },
  setlist: {
    update: "Could not update the set list",
    check: "Could not check the set list",
    load: "Could not load the set list",
    refresh: "Could not refresh the set list",
    add: "Could not add to the set list",
    saveCrate: "Could not save the set as a crate",
  },
  settings: {
    load: "Could not load settings, using defaults",
    save: "Could not save the setting",
    audioOutput: "Could not switch the audio output",
    folderPicker: "Could not open the folder picker",
    readModels: "Could not read the models",
    download: "Could not start the download",
    deleteModel: "Could not delete the model",
  },
  stems: {
    read: "Could not read the stems",
    split: "Could not split stems",
    downloadModel: "Could not download the stems model",
    volume: "Could not change the stem volume",
    mute: "Could not mute the stem",
    solo: "Could not solo the stem",
    export: "Could not export the stems",
  },
  ui: {
    openSettings: "Could not open Settings",
  },
} as const;
