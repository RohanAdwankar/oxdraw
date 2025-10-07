import { DiagramData, EdgeData, LayoutUpdate, NodeData, Point } from "./types";

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
