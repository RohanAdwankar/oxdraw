/* tslint:disable */
/* eslint-disable */
export class WasmEditorCore {
  free(): void;
  [Symbol.dispose](): void;
  nudgeNode(id: string, dx: number, dy: number): any;
  renderSvg(): string;
  setSource(source: string): void;
  viewModel(): any;
  cancelDrag(): void;
  endEdgeDrag(): any;
  endNodeDrag(): any;
  setBackground(background: string): void;
  beginEdgeDrag(id: string, index: number): void;
  beginNodeDrag(id: string, pointer_x: number, pointer_y: number): void;
  updateEdgeDrag(pointer_x: number, pointer_y: number): void;
  updateNodeDrag(pointer_x: number, pointer_y: number): void;
  endSubgraphDrag(): any;
  applyStyleUpdate(update: any): void;
  applyLayoutUpdate(update: any): void;
  beginSubgraphDrag(id: string, pointer_x: number, pointer_y: number): void;
  endGanttTaskDrag(): any;
  updateSubgraphDrag(pointer_x: number, pointer_y: number): void;
  beginGanttTaskDrag(id: string, mode: string, pointer_x: number): void;
  updateGanttTaskDrag(pointer_x: number): void;
  constructor(source: string, background: string);
  source(): string;
}

export type InitInput = RequestInfo | URL | Response | BufferSource | WebAssembly.Module;

export interface InitOutput {
  readonly memory: WebAssembly.Memory;
  readonly main: (a: number, b: number) => number;
  readonly __wbg_wasmeditorcore_free: (a: number, b: number) => void;
  readonly wasmeditorcore_applyLayoutUpdate: (a: number, b: any) => [number, number];
  readonly wasmeditorcore_applyStyleUpdate: (a: number, b: any) => [number, number];
  readonly wasmeditorcore_beginEdgeDrag: (a: number, b: number, c: number, d: number) => [number, number];
  readonly wasmeditorcore_beginGanttTaskDrag: (a: number, b: number, c: number, d: number, e: number, f: number) => [number, number];
  readonly wasmeditorcore_beginNodeDrag: (a: number, b: number, c: number, d: number, e: number) => [number, number];
  readonly wasmeditorcore_beginSubgraphDrag: (a: number, b: number, c: number, d: number, e: number) => [number, number];
  readonly wasmeditorcore_cancelDrag: (a: number) => void;
  readonly wasmeditorcore_endEdgeDrag: (a: number) => [number, number, number];
  readonly wasmeditorcore_endGanttTaskDrag: (a: number) => [number, number, number];
  readonly wasmeditorcore_endNodeDrag: (a: number) => [number, number, number];
  readonly wasmeditorcore_endSubgraphDrag: (a: number) => [number, number, number];
  readonly wasmeditorcore_new: (a: number, b: number, c: number, d: number) => [number, number, number];
  readonly wasmeditorcore_nudgeNode: (a: number, b: number, c: number, d: number, e: number) => [number, number, number];
  readonly wasmeditorcore_renderSvg: (a: number) => [number, number, number, number];
  readonly wasmeditorcore_setBackground: (a: number, b: number, c: number) => void;
  readonly wasmeditorcore_setSource: (a: number, b: number, c: number) => [number, number];
  readonly wasmeditorcore_source: (a: number) => [number, number, number, number];
  readonly wasmeditorcore_updateEdgeDrag: (a: number, b: number, c: number) => [number, number];
  readonly wasmeditorcore_updateGanttTaskDrag: (a: number, b: number) => [number, number];
  readonly wasmeditorcore_updateNodeDrag: (a: number, b: number, c: number) => [number, number];
  readonly wasmeditorcore_updateSubgraphDrag: (a: number, b: number, c: number) => [number, number];
  readonly wasmeditorcore_viewModel: (a: number) => [number, number, number];
  readonly __wbindgen_malloc: (a: number, b: number) => number;
  readonly __wbindgen_realloc: (a: number, b: number, c: number, d: number) => number;
  readonly __wbindgen_exn_store: (a: number) => void;
  readonly __externref_table_alloc: () => number;
  readonly __wbindgen_externrefs: WebAssembly.Table;
  readonly __externref_table_dealloc: (a: number) => void;
  readonly __wbindgen_free: (a: number, b: number, c: number) => void;
  readonly __wbindgen_start: () => void;
}

export type SyncInitInput = BufferSource | WebAssembly.Module;
/**
* Instantiates the given `module`, which can either be bytes or
* a precompiled `WebAssembly.Module`.
*
* @param {{ module: SyncInitInput }} module - Passing `SyncInitInput` directly is deprecated.
*
* @returns {InitOutput}
*/
export function initSync(module: { module: SyncInitInput } | SyncInitInput): InitOutput;

/**
* If `module_or_path` is {RequestInfo} or {URL}, makes a request and
* for everything else, calls `WebAssembly.instantiate` directly.
*
* @param {{ module_or_path: InitInput | Promise<InitInput> }} module_or_path - Passing `InitInput` directly is deprecated.
*
* @returns {Promise<InitOutput>}
*/
export default function __wbg_init (module_or_path?: { module_or_path: InitInput | Promise<InitInput> } | InitInput | Promise<InitInput>): Promise<InitOutput>;
