/// <reference types="vite/client" />

interface ImportMetaEnv {
  /** "1" builds the UI on mock data (browser demo, GitHub Pages). */
  readonly VITE_MOCK?: string;
}

interface ImportMeta {
  readonly env: ImportMetaEnv;
}
