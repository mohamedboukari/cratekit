/** Track browser, its context menu and the set list. */
export const browser = {
  label: "Track browser",
  allTracks: "All tracks",
  crate: "Crate",
  loading: "Loading...",
  summary_one: "{{count}} track · {{duration}}",
  summary_other: "{{count}} tracks · {{duration}}",
  noMatch: "No tracks match these filters",
  filters: {
    matchesKey: "Matches {{key}}",
    matchesDeckKey: "Matches deck key",
    nearBpm: "Near {{bpm}} BPM",
    nearDeckBpm: "Near deck BPM",
    hasStems: "Has stems",
  },
  search: {
    label: "Search tracks",
    placeholder: "Search title or artist",
  },
  table: {
    label_one: "{{crate}}, {{count}} track",
    label_other: "{{crate}}, {{count}} tracks",
    ready: "Ready",
    energy: "Energy {{value}} of 10",
    energyUnknown: "Energy unknown",
  },
  /** Sortable column headers, by column (see columns.ts). */
  columns: {
    title: "Title",
    artist: "Artist",
    bpm: "BPM",
    key: "Key",
    energy: "Energy",
    time: "Time",
  },
  /** Tags in the Ready column. */
  tags: {
    busy: "{{kind}} {{percent}}%",
    activity: "{{kind}}...",
    analyzed: "analyzed",
    notAnalyzed: "not analyzed",
    stems: "stems",
    mastered: "mastered",
    /** Job kinds, as Rust names them. */
    jobs: {
      analysis: "analysis",
      stems: "stems",
      master: "master",
      export: "export",
      "model-download": "model-download",
    },
  },
  empty: {
    title: "Drop tracks or a folder",
    hint: "{{formats}}. Your files stay where they are; Cratekit only reads them.",
    adding: "Adding...",
    addTracks: "Add tracks...",
    addFolder: "Add folder...",
  },
  /** `{{suffix}}` is " (3)" when the action applies to several tracks, else empty. */
  contextMenu: {
    loadOnDeck: "Load on deck",
    addToSetList: "Add to set list{{suffix}}",
    analyze: "Analyze{{suffix}}",
    splitStems: "Split stems",
    useAsReference: "Use as mastering reference",
    exportToVirtualDj: "Export to VirtualDJ{{suffix}}...",
    addToCrate: "Add to crate",
    showInFolder: "Show in folder",
    removeFromLibrary: "Remove from library",
    comingSoon: "Arrives with the library task",
  },
  /** The label that follows the pointer when several tracks are dragged. */
  dragTracks_one: "{{count}} track",
  dragTracks_other: "{{count}} tracks",
  setList: {
    title: "Set list",
    saveAsCrate: "Save as crate",
    clear: "Clear",
    crateName: "Crate name",
    save: "Save",
    saved: 'Saved "{{name}}" as a crate',
    empty:
      "Drag tracks here to build your set, or right-click a track and choose Add to set list",
    order: "Set order",
    dropAtEnd: "Drop here to add at the end",
    keyClash: "key clash with the previous track",
    bpmJump: "BPM {{change}}%",
    itemLabel: "{{position}}. {{title}}, load on deck",
    remove: "Remove {{title}} from the set list",
  },
  errors: {
    filePicker: "Could not open the file picker",
    analyze: "Could not start the analysis",
    splitStems: "Could not split stems",
    addToCrate: "Could not add to {{crate}}",
  },
} as const;
