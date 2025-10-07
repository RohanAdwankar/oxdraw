/// <reference types="vite/client" />

declare interface ImportMetaEnv {
  readonly VITE_OXDRAW_API?: string;
}

declare interface ImportMeta {
  readonly env: ImportMetaEnv;
}
