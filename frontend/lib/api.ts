import {
  DiagramData,
  DiagramSummary,
  EdgeStyleUpdate,
  LayoutUpdate,
  NodeStyleUpdate,
  StyleUpdate,
  CodeMapMapping,
} from "./types";

const API_BASE = process.env.NEXT_PUBLIC_OXDRAW_API ?? "";

interface SessionResponse {
  sessionId: string;
  diagrams: DiagramSummary[];
  currentDiagramId?: number;
}

export async function getCurrentSession(): Promise<SessionResponse> {
  const response = await fetch(`${API_BASE}/api/sessions/current`, {
    method: "GET",
    cache: "no-store",
  });

  if (!response.ok) {
    throw new Error(`Failed to get session: ${response.status}`);
  }

  return response.json();
}

export async function listDiagrams(): Promise<DiagramSummary[]> {
  const response = await fetch(`${API_BASE}/api/diagrams`, {
    method: "GET",
    cache: "no-store",
  });

  if (!response.ok) {
    throw new Error(`Failed to list diagrams: ${response.status}`);
  }

  return response.json();
}

export async function createDiagram(name: string, template?: string): Promise<DiagramData> {
  const response = await fetch(`${API_BASE}/api/diagrams`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ name, template }),
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to create diagram: ${response.status}`);
  }

  return response.json();
}

export async function fetchDiagram(diagramId: number | string | null): Promise<DiagramData> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  const response = await fetch(`${API_BASE}/api/diagrams/${diagramId}`, {
    method: "GET",
    cache: "no-store",
  });

  if (!response.ok) {
    throw new Error(`Failed to fetch diagram: ${response.status}`);
  }

  return response.json() as Promise<DiagramData>;
}

export async function fetchDiagramSvg(diagramId: number | string | null): Promise<string> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  const response = await fetch(`${API_BASE}/api/diagrams/${diagramId}/svg`, {
    method: "GET",
    cache: "no-store",
  });

  if (!response.ok) {
    throw new Error(`Failed to fetch diagram SVG: ${response.status}`);
  }

  return response.text();
}

export async function updateDiagramContent(diagramId: number | string | null, content: string): Promise<void> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  const response = await fetch(`${API_BASE}/api/diagrams/${diagramId}/content`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ content }),
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to update diagram: ${response.status}`);
  }
}

export async function updateDiagramName(diagramId: number | string | null, name: string): Promise<void> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  const response = await fetch(`${API_BASE}/api/diagrams/${diagramId}/name`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ name }),
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to rename diagram: ${response.status}`);
  }
}

export async function duplicateDiagram(diagramId: number | string | null, newName?: string): Promise<DiagramData> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  const response = await fetch(`${API_BASE}/api/diagrams/${diagramId}/duplicate`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ name: newName }),
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to duplicate diagram: ${response.status}`);
  }

  return response.json();
}

export async function deleteDiagram(diagramId: number | string | null): Promise<void> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  const response = await fetch(`${API_BASE}/api/diagrams/${diagramId}`, {
    method: "DELETE",
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to delete diagram: ${response.status}`);
  }
}

export async function updateLayout(diagramId: number | string | null, update: LayoutUpdate): Promise<void> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  const response = await fetch(`${API_BASE}/api/diagrams/${diagramId}/layout`, {
    method: "PUT",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify(update),
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to update layout: ${response.status}`);
  }
}

export async function updateStyle(diagramId: number | string | null, update: StyleUpdate): Promise<void> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
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

  if (Object.keys(payload).length === 0) {
    return;
  }

  const response = await fetch(`${API_BASE}/api/diagrams/${diagramId}/style`, {
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

export async function updateSource(diagramId: number | string | null, source: string): Promise<void> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  const response = await fetch(`${API_BASE}/api/diagrams/${diagramId}/source`, {
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

export async function deleteNode(diagramId: number | string | null, nodeId: string): Promise<void> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  const response = await fetch(
    `${API_BASE}/api/diagrams/${diagramId}/nodes/${encodeURIComponent(nodeId)}`,
    {
      method: "DELETE",
    }
  );

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to delete node: ${response.status}`);
  }
}

export async function deleteEdge(diagramId: number | string | null, edgeId: string): Promise<void> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  const response = await fetch(
    `${API_BASE}/api/diagrams/${diagramId}/edges/${encodeURIComponent(edgeId)}`,
    {
      method: "DELETE",
    }
  );

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to delete edge: ${response.status}`);
  }
}

export async function updateNodeImage(
  diagramId: number | string | null,
  nodeId: string,
  payload: ({ mimeType?: string; data?: string | null; padding?: number } | null)
): Promise<void> {
  if (diagramId === null) {
    throw new Error("No diagram selected");
  }
  let body: Record<string, unknown>;

  if (payload === null) {
    body = { data: null };
  } else {
    body = {};
    if (payload.mimeType !== undefined) {
      body.mime_type = payload.mimeType;
    }
    if (payload.data !== undefined) {
      body.data = payload.data;
    }
    if (payload.padding !== undefined) {
      body.padding = payload.padding;
    }
    if (Object.keys(body).length === 0) {
      return;
    }
  }

  const response = await fetch(
    `${API_BASE}/api/diagrams/${diagramId}/nodes/${encodeURIComponent(nodeId)}/image`,
    {
      method: "PUT",
      headers: {
        "Content-Type": "application/json",
      },
      body: JSON.stringify(body),
    }
  );

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to update node image: ${response.status}`);
  }
}

export async function fetchCodeMapMapping(): Promise<CodeMapMapping | null> {
  const response = await fetch(`${API_BASE}/api/codemap/mapping`);
  if (!response.ok) {
    throw new Error(`Failed to fetch code map mapping: ${response.statusText}`);
  }
  return response.json();
}

export async function fetchCodeMapFile(path: string): Promise<string> {
  const response = await fetch(`${API_BASE}/api/codemap/file?path=${encodeURIComponent(path)}`);
  if (!response.ok) {
    throw new Error(`Failed to fetch file content: ${response.statusText}`);
  }
  return response.text();
}

export async function openInEditor(path: string, line: number | undefined, editor: "vscode" | "nvim"): Promise<void> {
  const response = await fetch(`${API_BASE}/api/codemap/open`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
    },
    body: JSON.stringify({ path, line, editor }),
  });

  if (!response.ok) {
    const message = await response.text();
    throw new Error(message || `Failed to open editor: ${response.status}`);
  }
}
