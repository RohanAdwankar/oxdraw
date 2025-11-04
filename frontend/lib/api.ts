import {
  DiagramData,
  EdgeStyleUpdate,
  LayoutUpdate,
  NodeStyleUpdate,
  StyleUpdate,
} from "./types";

const API_BASE = process.env.NEXT_PUBLIC_OXDRAW_API ?? "http://127.0.0.1:5151";

export async function fetchDiagram(): Promise<DiagramData> {
  const response = await fetch(`${API_BASE}/api/diagram`, {
    method: "GET",
    cache: "no-store",
  });

  if (!response.ok) {
    throw new Error(`Failed to load diagram: ${response.status}`);
  }

  return (await response.json()) as DiagramData;
}

export async function updateLayout(update: LayoutUpdate): Promise<void> {
  const response = await fetch(`${API_BASE}/api/diagram/layout`, {
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

export async function updateSource(source: string): Promise<void> {
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

  if (Object.keys(payload).length === 0) {
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

export async function updateNodeImage(
  nodeId: string,
  payload: { mimeType: string; data: string } | null
): Promise<void> {
  const body = payload
    ? { mime_type: payload.mimeType, data: payload.data }
    : { data: null };

  const response = await fetch(
    `${API_BASE}/api/diagram/nodes/${encodeURIComponent(nodeId)}/image`,
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
