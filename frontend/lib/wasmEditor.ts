export interface WasmEditorCore {
  renderSvg(): string;
  viewModel(): unknown;
  beginNodeDrag(id: string, pointerX: number, pointerY: number): void;
  updateNodeDrag(pointerX: number, pointerY: number): void;
  endNodeDrag(): unknown;
  beginEdgeDrag(id: string, index: number): void;
  updateEdgeDrag(pointerX: number, pointerY: number): void;
  endEdgeDrag(): unknown;
  beginSubgraphDrag(id: string, pointerX: number, pointerY: number): void;
  updateSubgraphDrag(pointerX: number, pointerY: number): void;
  endSubgraphDrag(): unknown;
  beginGanttTaskDrag(id: string, mode: string, pointerX: number): void;
  updateGanttTaskDrag(pointerX: number): void;
  endGanttTaskDrag(): unknown;
  cancelDrag(): void;
  nudgeNode(id: string, dx: number, dy: number): unknown;
  source(): string;
  applyLayoutUpdate(update: unknown): void;
  applyStyleUpdate(update: unknown): void;
  setSource(source: string): void;
  deleteNode(id: string): boolean;
  deleteEdge(id: string): boolean;
}

interface WasmModule {
  default: (
    initInput?:
      | string
      | URL
      | Request
      | { module_or_path?: string | URL | Request; moduleOrPath?: string | URL | Request }
  ) => Promise<unknown>;
  WasmEditorCore: new (source: string, background: string) => WasmEditorCore;
}

let modulePromise: Promise<WasmModule> | null = null;
let initPromise: Promise<unknown> | null = null;

function withBasePath(path: string): string {
  const base = process.env.NEXT_PUBLIC_BASE_PATH ?? "";
  if (!base) {
    return path;
  }
  const normalizedBase = base.endsWith("/") ? base.slice(0, -1) : base;
  const normalizedPath = path.startsWith("/") ? path : `/${path}`;
  return `${normalizedBase}${normalizedPath}`;
}

async function loadWasmModule(): Promise<WasmModule> {
  if (!modulePromise) {
    const dynamicImport = new Function(
      "path",
      "return import(/* webpackIgnore: true */ path);"
    ) as (path: string) => Promise<WasmModule>;
    modulePromise = dynamicImport(withBasePath("/oxdraw_wasm.js"));
  }
  return modulePromise;
}

export async function createWasmEditor(
  source: string,
  background: string
): Promise<WasmEditorCore> {
  const wasm = await loadWasmModule();
  if (!initPromise) {
    initPromise = wasm
      .default({ module_or_path: withBasePath("/oxdraw_wasm_bg.wasm") })
      .catch(() => wasm.default(withBasePath("/oxdraw_wasm_bg.wasm")));
  }
  await initPromise;
  return new wasm.WasmEditorCore(source, background);
}
