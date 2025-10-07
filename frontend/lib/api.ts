import {
  DiagramData,
  EdgeData,
  EdgeStyleUpdate,
  LayoutUpdate,
  NodeData,
  NodeStyleUpdate,
  Point,
  StyleUpdate,
} from "./types";

const API_BASE = process.env.NEXT_PUBLIC_OXDRAW_API ?? "http://127.0.0.1:5151";

interface RawPoint {
  x: number;
  y: number;
}

interface RawNode {
  id: string;
  label: string;
  shape: string;
  auto_position: RawPoint;
  rendered_position: RawPoint;
  position?: RawPoint | null;
  fill_color?: string | null;
  stroke_color?: string | null;
  text_color?: string | null;
}

interface RawEdge {
  id: string;
  from: string;
  to: string;
  label?: string;
  kind: string;
  auto_points: RawPoint[];
  rendered_points: RawPoint[];
  points?: RawPoint[] | null;
  color?: string | null;
  arrow_direction?: string | null;
}

interface RawSize {
  width: number;
  height: number;
}

interface RawDiagram {
  source_path: string;
  background: string;
  auto_size: RawSize;
  render_size: RawSize;
  nodes: RawNode[];
  edges: RawEdge[];
  source: string;
}

function mapPoint(point: RawPoint): Point {
  return { x: point.x, y: point.y };
}

function mapNode(raw: RawNode): NodeData {
  return {
    id: raw.id,
    label: raw.label,
    shape: raw.shape as NodeData["shape"],
    autoPosition: mapPoint(raw.auto_position),
    renderedPosition: mapPoint(raw.rendered_position),
    overridePosition: raw.position ? mapPoint(raw.position) : undefined,
    fillColor: raw.fill_color ?? undefined,
    strokeColor: raw.stroke_color ?? undefined,
    textColor: raw.text_color ?? undefined,
  };
}

function mapEdge(raw: RawEdge): EdgeData {
  return {
    id: raw.id,
    from: raw.from,
    to: raw.to,
    label: raw.label,
    kind: raw.kind as EdgeData["kind"],
    autoPoints: raw.auto_points.map(mapPoint),
    renderedPoints: raw.rendered_points.map(mapPoint),
    overridePoints: raw.points ? raw.points.map(mapPoint) : undefined,
    color: raw.color ?? undefined,
    arrowDirection: raw.arrow_direction ? (raw.arrow_direction as EdgeData["arrowDirection"]) : undefined,
  };
}

export async function fetchDiagram(): Promise<DiagramData> {
  const response = await fetch(`${API_BASE}/api/diagram`, {
    method: "GET",
    cache: "no-store",
  });

  if (!response.ok) {
    throw new Error(`Failed to load diagram: ${response.status}`);
  }

  const payload = (await response.json()) as RawDiagram;
  return {
    sourcePath: payload.source_path,
    background: payload.background,
    autoSize: {
      width: payload.auto_size.width,
      height: payload.auto_size.height,
    },
    renderSize: {
      width: payload.render_size.width,
      height: payload.render_size.height,
    },
    nodes: payload.nodes.map(mapNode),
    edges: payload.edges.map(mapEdge),
    source: payload.source,
  };
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
