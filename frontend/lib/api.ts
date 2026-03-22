import {
  DiagramData,
  EdgeStyleUpdate,
  LayoutUpdate,
  NodeStyleUpdate,
  StyleUpdate,
  CodeMapMapping,
  SearchResult,
} from "./types";
import { createWasmEditor, type WasmEditorCore } from "./wasmEditor";

type Mode = "server" | "local";
const MODE: Mode = process.env.NEXT_PUBLIC_OXDRAW_MODE === "local" ? "local" : "server";

const API_BASE = process.env.NEXT_PUBLIC_OXDRAW_API ?? "http://127.0.0.1:5151";
const LOCAL_SOURCE_KEY = "oxdraw.local.source.v1";
const LOCAL_BACKGROUND_KEY = "oxdraw.local.background.v1";
const LOCAL_SOURCE_PATH = "playground.mmd";
const LOCAL_SHARE_PARAM = "d";
const LOCAL_SAMPLE_SOURCE = `graph TD
    A[Client] --> B{Payload valid?}
    B -->|Yes| C[Queue job]
    B -->|No| D[Show error]
    C --> E[Process]
    E --> F{Needs retry?}
    F -->|Yes| C
    F -->|No| G[Done]
`;

let localCorePromise: Promise<WasmEditorCore> | null = null;
let localCore: WasmEditorCore | null = null;

const textEncoder = new TextEncoder();
const textDecoder = new TextDecoder();

function isBrowser(): boolean {
  return typeof window !== "undefined";
}

function bytesToBase64Url(bytes: Uint8Array): string {
  let binary = "";
  const chunkSize = 0x8000;
  for (let index = 0; index < bytes.length; index += chunkSize) {
    const chunk = bytes.subarray(index, index + chunkSize);
    binary += String.fromCharCode(...chunk);
  }
  return btoa(binary).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/g, "");
}

function base64UrlToBytes(value: string): Uint8Array {
  const base64 = value.replace(/-/g, "+").replace(/_/g, "/");
  const padded = base64 + "=".repeat((4 - (base64.length % 4 || 4)) % 4);
  const binary = atob(padded);
  const bytes = new Uint8Array(binary.length);
  for (let index = 0; index < binary.length; index += 1) {
    bytes[index] = binary.charCodeAt(index);
  }
  return bytes;
}

function toArrayBuffer(bytes: Uint8Array): ArrayBuffer {
  return bytes.buffer.slice(bytes.byteOffset, bytes.byteOffset + bytes.byteLength) as ArrayBuffer;
}

async function gzipBytes(bytes: Uint8Array): Promise<Uint8Array | null> {
  if (!isBrowser() || typeof CompressionStream === "undefined") {
    return null;
  }
  const stream = new Blob([toArrayBuffer(bytes)]).stream().pipeThrough(new CompressionStream("gzip"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

async function gunzipBytes(bytes: Uint8Array): Promise<Uint8Array> {
  if (!isBrowser() || typeof DecompressionStream === "undefined") {
    throw new Error("This browser cannot open compressed shared diagrams.");
  }
  const stream = new Blob([toArrayBuffer(bytes)]).stream().pipeThrough(new DecompressionStream("gzip"));
  return new Uint8Array(await new Response(stream).arrayBuffer());
}

async function encodeLocalShareSource(source: string): Promise<string> {
  const plainBytes = textEncoder.encode(source);
  const gzippedBytes = await gzipBytes(plainBytes);
  if (gzippedBytes && gzippedBytes.length < plainBytes.length) {
    return `gz.${bytesToBase64Url(gzippedBytes)}`;
  }
  return `txt.${bytesToBase64Url(plainBytes)}`;
}

async function decodeLocalShareSource(token: string): Promise<string> {
  const trimmed = token.trim();
  if (!trimmed) {
    throw new Error("Shared diagram URL is empty.");
  }

  const separatorIndex = trimmed.indexOf(".");
  const prefix = separatorIndex === -1 ? "txt" : trimmed.slice(0, separatorIndex);
  const payload = separatorIndex === -1 ? trimmed : trimmed.slice(separatorIndex + 1);
  const bytes = base64UrlToBytes(payload);

  if (prefix === "gz") {
    const uncompressed = await gunzipBytes(bytes);
    return textDecoder.decode(uncompressed);
  }

  return textDecoder.decode(bytes);
}

function readLocalShareTokenFromUrl(): string | null {
  if (!isBrowser()) {
    return null;
  }
  const rawHash = window.location.hash.startsWith("#")
    ? window.location.hash.slice(1)
    : window.location.hash;
  if (!rawHash) {
    return null;
  }
  const params = new URLSearchParams(rawHash);
  return params.get(LOCAL_SHARE_PARAM);
}

function readLocalStorage(key: string): string | null {
  if (!isBrowser()) {
    return null;
  }
  try {
    return window.localStorage.getItem(key);
  } catch {
    return null;
  }
}

function writeLocalStorage(key: string, value: string): void {
  if (!isBrowser()) {
    return;
  }
  try {
    window.localStorage.setItem(key, value);
  } catch {
    // ignore quota/private mode failures
  }
}

async function ensureLocalCore(): Promise<WasmEditorCore> {
  if (localCore) {
    return localCore;
  }
  if (!localCorePromise) {
    const background = readLocalStorage(LOCAL_BACKGROUND_KEY) ?? "white";
    localCorePromise = (async () => {
      let source = readLocalStorage(LOCAL_SOURCE_KEY) ?? LOCAL_SAMPLE_SOURCE;
      const sharedToken = readLocalShareTokenFromUrl();
      if (sharedToken) {
        try {
          source = await decodeLocalShareSource(sharedToken);
          writeLocalStorage(LOCAL_SOURCE_KEY, source);
        } catch (error) {
          console.warn("Failed to decode shared diagram URL.", error);
        }
      }
      return createWasmEditor(source, background);
    })().then((core) => {
      localCore = core;
      return core;
    });
  }
  return localCorePromise;
}

function localDiagramFromCore(core: WasmEditorCore): DiagramData {
  const vm = core.viewModel() as Record<string, unknown>;
  return {
    sourcePath: LOCAL_SOURCE_PATH,
    kind: (vm.kind as "flowchart" | "gantt") ?? "flowchart",
    background: (vm.background as string) ?? "white",
    autoSize: (vm.autoSize as { width: number; height: number }) ?? { width: 0, height: 0 },
    renderSize: (vm.renderSize as { width: number; height: number }) ?? { width: 0, height: 0 },
    nodes: (vm.nodes as DiagramData["nodes"]) ?? [],
    edges: (vm.edges as DiagramData["edges"]) ?? [],
    subgraphs: (vm.subgraphs as DiagramData["subgraphs"]) ?? [],
    gantt: (vm.gantt as DiagramData["gantt"]) ?? undefined,
    source: (vm.source as string) ?? "",
  };
}

function toLocalLayoutPayload(update: LayoutUpdate): Record<string, unknown> {
  const payload: Record<string, unknown> = {};
  if (update.nodes && Object.keys(update.nodes).length > 0) {
    payload.nodes = update.nodes;
  }
  if (update.edges && Object.keys(update.edges).length > 0) {
    payload.edges = update.edges;
  }
  if (update.ganttTasks && Object.keys(update.ganttTasks).length > 0) {
    const entries: Array<[string, { start_day?: number; end_day?: number } | null]> = [];
    for (const [taskId, value] of Object.entries(update.ganttTasks)) {
      if (value === null) {
        entries.push([taskId, null]);
        continue;
      }
      const patch: { start_day?: number; end_day?: number } = {};
      if (value.startDay !== undefined) {
        patch.start_day = value.startDay;
      }
      if (value.endDay !== undefined) {
        patch.end_day = value.endDay;
      }
      entries.push([taskId, patch]);
    }
    payload.gantt_tasks = Object.fromEntries(entries);
  }
  return payload;
}

function toLocalStylePayload(update: StyleUpdate): Record<string, unknown> {
  const payload: Record<string, unknown> = {};

  const nodeEntries: Array<[string, Record<string, string | null> | null]> = [];
  for (const [key, value] of Object.entries(update.nodeStyles ?? {})) {
    const normalized = normalizeNodeStyle(value);
    if (normalized !== undefined) {
      nodeEntries.push([key, normalized]);
    }
  }
  if (nodeEntries.length > 0) {
    payload["node_styles"] = Object.fromEntries(nodeEntries);
  }

  const edgeEntries: Array<[string, Record<string, string | null> | null]> = [];
  for (const [key, value] of Object.entries(update.edgeStyles ?? {})) {
    const normalized = normalizeEdgeStyle(value);
    if (normalized !== undefined) {
      edgeEntries.push([key, normalized]);
    }
  }
  if (edgeEntries.length > 0) {
    payload["edge_styles"] = Object.fromEntries(edgeEntries);
  }

  if (update.ganttStyle) {
    const ganttPatch: Record<string, string | null> = {};
    if (update.ganttStyle.rowFillEven !== undefined) {
      ganttPatch.row_fill_even = update.ganttStyle.rowFillEven;
    }
    if (update.ganttStyle.rowFillOdd !== undefined) {
      ganttPatch.row_fill_odd = update.ganttStyle.rowFillOdd;
    }
    if (update.ganttStyle.taskFill !== undefined) {
      ganttPatch.task_fill = update.ganttStyle.taskFill;
    }
    if (update.ganttStyle.milestoneFill !== undefined) {
      ganttPatch.milestone_fill = update.ganttStyle.milestoneFill;
    }
    if (update.ganttStyle.taskText !== undefined) {
      ganttPatch.task_text = update.ganttStyle.taskText;
    }
    if (update.ganttStyle.milestoneText !== undefined) {
      ganttPatch.milestone_text = update.ganttStyle.milestoneText;
    }
    if (Object.keys(ganttPatch).length > 0) {
      payload["gantt_style"] = ganttPatch;
    }
  }

  return payload;
}

function persistLocalCore(core: WasmEditorCore, overrideSource?: string): void {
  const source = overrideSource ?? core.source();
  writeLocalStorage(LOCAL_SOURCE_KEY, source);
}

export function isLocalMode(): boolean {
  return MODE === "local";
}

export async function buildLocalShareUrl(source: string): Promise<string> {
  if (MODE !== "local") {
    throw new Error("Share links are only available in local mode.");
  }
  if (!isBrowser()) {
    throw new Error("Share links require a browser environment.");
  }

  const token = await encodeLocalShareSource(source);
  const url = new URL(window.location.href);
  url.hash = `${LOCAL_SHARE_PARAM}=${token}`;
  return url.toString();
}

export async function searchCodebase(query: string): Promise<SearchResult[]> {
  if (MODE === "local") {
    return [];
  }
  const response = await fetch(`${API_BASE}/api/codemap/search?query=${encodeURIComponent(query)}`);
  if (!response.ok) {
    const text = await response.text();
    console.error("Search failed:", text);
    throw new Error(`Failed to search codebase: ${response.statusText}`);
  }
  return response.json();
}

export async function fetchDiagram(): Promise<DiagramData> {
  if (MODE === "local") {
    const core = await ensureLocalCore();
    return localDiagramFromCore(core);
  }

  const response = await fetch(`${API_BASE}/api/diagram`, {
    method: "GET",
    cache: "no-store",
  });

  if (!response.ok) {
    const text = await response.text();
    console.error("Fetch diagram failed:", text);
    throw new Error(`Failed to load diagram: ${response.status}`);
  }

  return (await response.json()) as DiagramData;
}

export async function updateLayout(update: LayoutUpdate): Promise<void> {
  const payload: Record<string, unknown> = {};
  if (update.nodes && Object.keys(update.nodes).length > 0) {
    payload.nodes = update.nodes;
  }
  if (update.edges && Object.keys(update.edges).length > 0) {
    payload.edges = update.edges;
  }
  if (update.ganttTasks && Object.keys(update.ganttTasks).length > 0) {
    const entries: Array<[string, { start_day?: number; end_day?: number } | null]> = [];
    for (const [taskId, value] of Object.entries(update.ganttTasks)) {
      if (value === null) {
        entries.push([taskId, null]);
        continue;
      }
      const taskPatch: { start_day?: number; end_day?: number } = {};
      if (value.startDay !== undefined) {
        taskPatch.start_day = value.startDay;
      }
      if (value.endDay !== undefined) {
        taskPatch.end_day = value.endDay;
      }
      entries.push([taskId, taskPatch]);
    }
    payload.gantt_tasks = Object.fromEntries(entries);
  }

  if (Object.keys(payload).length === 0) {
    return;
  }

  if (MODE === "local") {
    const core = await ensureLocalCore();
    core.applyLayoutUpdate(toLocalLayoutPayload(update));
    persistLocalCore(core);
    return;
  }

  const response = await fetch(`${API_BASE}/api/diagram/layout`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(payload),
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to update layout: ${response.status}`);
  }
}

export async function updateSource(source: string): Promise<void> {
  if (MODE === "local") {
    const core = await ensureLocalCore();
    core.setSource(source);
    persistLocalCore(core, source);
    return;
  }

  const response = await fetch(`${API_BASE}/api/diagram/source`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ source }),
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to update source: ${response.status}`);
  }
}

export async function updateStyle(update: StyleUpdate): Promise<void> {
  const payload: Record<string, unknown> = {};

  const nodeEntries: Array<[string, Record<string, string | null> | null]> = [];
  for (const [key, value] of Object.entries(update.nodeStyles ?? {})) {
    const normalized = normalizeNodeStyle(value);
    if (normalized !== undefined) {
      nodeEntries.push([key, normalized]);
    }
  }
  if (nodeEntries.length > 0) {
    payload["node_styles"] = Object.fromEntries(nodeEntries);
  }

  const edgeEntries: Array<[string, Record<string, string | null> | null]> = [];
  for (const [key, value] of Object.entries(update.edgeStyles ?? {})) {
    const normalized = normalizeEdgeStyle(value);
    if (normalized !== undefined) {
      edgeEntries.push([key, normalized]);
    }
  }
  if (edgeEntries.length > 0) {
    payload["edge_styles"] = Object.fromEntries(edgeEntries);
  }

  if (update.ganttStyle) {
    const ganttPatch: Record<string, string | null> = {};
    if (update.ganttStyle.rowFillEven !== undefined) {
      ganttPatch.row_fill_even = update.ganttStyle.rowFillEven;
    }
    if (update.ganttStyle.rowFillOdd !== undefined) {
      ganttPatch.row_fill_odd = update.ganttStyle.rowFillOdd;
    }
    if (update.ganttStyle.taskFill !== undefined) {
      ganttPatch.task_fill = update.ganttStyle.taskFill;
    }
    if (update.ganttStyle.milestoneFill !== undefined) {
      ganttPatch.milestone_fill = update.ganttStyle.milestoneFill;
    }
    if (update.ganttStyle.taskText !== undefined) {
      ganttPatch.task_text = update.ganttStyle.taskText;
    }
    if (update.ganttStyle.milestoneText !== undefined) {
      ganttPatch.milestone_text = update.ganttStyle.milestoneText;
    }
    if (Object.keys(ganttPatch).length > 0) {
      payload["gantt_style"] = ganttPatch;
    }
  }

  if (Object.keys(payload).length === 0) {
    return;
  }

  if (MODE === "local") {
    const core = await ensureLocalCore();
    core.applyStyleUpdate(toLocalStylePayload(update));
    persistLocalCore(core);
    return;
  }

  const response = await fetch(`${API_BASE}/api/diagram/style`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(payload),
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to update style: ${response.status}`);
  }
}

export async function updateNodeImage(
  nodeId: string,
  image: { mimeType?: string; data?: string; padding?: number } | null
): Promise<void> {
  if (MODE === "local") {
    throw new Error("Node image upload is not available in local mode yet.");
  }

  const payload =
    image === null
      ? null
      : {
          ...(image.mimeType !== undefined ? { mime_type: image.mimeType } : {}),
          ...(image.data !== undefined ? { data: image.data } : {}),
          ...(image.padding !== undefined ? { padding: image.padding } : {}),
        };

  const response = await fetch(
    `${API_BASE}/api/diagram/nodes/${encodeURIComponent(nodeId)}/image`,
    {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(payload),
    }
  );

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to update node image: ${response.status}`);
  }
}

function normalizeNodeStyle(
  style: NodeStyleUpdate | null | undefined
): Record<string, string | null> | null | undefined {
  if (style === null) {
    return null;
  }
  if (style === undefined) {
    return undefined;
  }

  const patch: Record<string, string | null> = {};
  if (style.fill !== undefined) {
    patch.fill = style.fill;
  }
  if (style.stroke !== undefined) {
    patch.stroke = style.stroke;
  }
  if (style.text !== undefined) {
    patch.text = style.text;
  }
  if (style.labelFill !== undefined) {
    patch["label_fill"] = style.labelFill;
  }
  if (style.imageFill !== undefined) {
    patch["image_fill"] = style.imageFill;
  }

  return Object.keys(patch).length > 0 ? patch : undefined;
}

function normalizeEdgeStyle(
  style: EdgeStyleUpdate | null | undefined
): Record<string, string | null> | null | undefined {
  if (style === null) {
    return null;
  }
  if (style === undefined) {
    return undefined;
  }

  const patch: Record<string, string | null> = {};
  if (style.line !== undefined) {
    patch.line = style.line;
  }
  if (style.color !== undefined) {
    patch.color = style.color;
  }
  if (style.arrow !== undefined) {
    patch.arrow = style.arrow;
  }

  return Object.keys(patch).length > 0 ? patch : undefined;
}

export async function deleteNode(nodeId: string): Promise<void> {
  if (MODE === "local") {
    const core = await ensureLocalCore();
    const deleted = core.deleteNode(nodeId);
    if (!deleted) {
      throw new Error(`Node not found: ${nodeId}`);
    }
    persistLocalCore(core);
    return;
  }

  const response = await fetch(
    `${API_BASE}/api/diagram/nodes/${encodeURIComponent(nodeId)}`,
    {
      method: "DELETE",
    }
  );

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to delete node: ${response.status}`);
  }
}

export async function deleteEdge(edgeId: string): Promise<void> {
  if (MODE === "local") {
    const core = await ensureLocalCore();
    const deleted = core.deleteEdge(edgeId);
    if (!deleted) {
      throw new Error(`Edge not found: ${edgeId}`);
    }
    persistLocalCore(core);
    return;
  }

  const response = await fetch(
    `${API_BASE}/api/diagram/edges/${encodeURIComponent(edgeId)}`,
    {
      method: "DELETE",
    }
  );

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to delete edge: ${response.status}`);
  }
}

export async function fetchCodeMapMapping(): Promise<CodeMapMapping> {
  if (MODE === "local") {
    throw new Error("Code map is not available in local mode.");
  }
  const response = await fetch(`${API_BASE}/api/codemap/mapping`, {
    method: "GET",
    cache: "no-store",
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `Failed to load code map mapping: ${response.status}`);
  }

  return (await response.json()) as CodeMapMapping;
}

export async function fetchCodeMapFile(path: string): Promise<string> {
  if (MODE === "local") {
    throw new Error("Code map is not available in local mode.");
  }
  const response = await fetch(
    `${API_BASE}/api/codemap/file?path=${encodeURIComponent(path)}`,
    {
      method: "GET",
      cache: "no-store",
    }
  );

  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `Failed to load code map file: ${response.status}`);
  }

  return await response.text();
}

export async function openInEditor(file: string, line?: number, editor: string = "vscode"): Promise<void> {
  if (MODE === "local") {
    throw new Error("Open in editor is not available in local mode.");
  }
  const response = await fetch(`${API_BASE}/api/open-in-editor`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ file, line, editor }),
  });

  if (!response.ok) {
    const text = await response.text();
    throw new Error(text || `Failed to open file in editor: ${response.status}`);
  }
}

export function __debugLocalSnapshot(): Record<string, unknown> | null {
  if (MODE !== "local" || !localCore) {
    return null;
  }
  return {
    source: localCore.source(),
    viewModel: localCore.viewModel(),
  };
}
