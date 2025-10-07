export type NodeShape = "rectangle" | "stadium" | "circle" | "diamond";
export type EdgeKind = "solid" | "dashed";

export interface Point {
  x: number;
  y: number;
}

export interface Size {
  width: number;
  height: number;
}

export interface NodeData {
  id: string;
  label: string;
  shape: NodeShape;
  autoPosition: Point;
  renderedPosition: Point;
  overridePosition?: Point;
}

export interface EdgeData {
  id: string;
  from: string;
  to: string;
  label?: string;
  kind: EdgeKind;
  autoPoints: Point[];
  renderedPoints: Point[];
  overridePoints?: Point[];
}

export interface DiagramData {
  sourcePath: string;
  background: string;
  autoSize: Size;
  renderSize: Size;
  nodes: NodeData[];
  edges: EdgeData[];
}

export interface LayoutUpdate {
  nodes?: Record<string, Point | null>;
  edges?: Record<string, { points?: Point[] | null }>;
}
