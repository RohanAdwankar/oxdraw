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

async function loadWasmModule(): Promise<WasmModule> {
  if (!modulePromise) {
    const dynamicImport = new Function(
      "path",
      "return import(/* webpackIgnore: true */ path);"
    ) as (path: string) => Promise<WasmModule>;
    modulePromise = dynamicImport("/oxdraw_wasm.js");
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
      .default({ module_or_path: "/oxdraw_wasm_bg.wasm" })
      .catch(() => wasm.default("/oxdraw_wasm_bg.wasm"));
  }
  await initPromise;
  return new wasm.WasmEditorCore(source, background);
}
