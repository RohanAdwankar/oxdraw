export type NodeShape =
  | "rectangle"
  | "stadium"
  | "circle"
  | "double-circle"
  | "diamond"
  | "subroutine"
  | "cylinder"
  | "hexagon"
  | "parallelogram"
  | "parallelogram-alt"
  | "trapezoid"
  | "trapezoid-alt"
  | "asymmetric";
export type EdgeKind = "solid" | "dashed";
export type EdgeArrowDirection = "forward" | "backward" | "both" | "none";

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
  fillColor?: string;
  strokeColor?: string;
  textColor?: string;
  membership?: string[];
  image?: NodeImageData;
  width: number;
  height: number;
}

export interface NodeImageData {
  mimeType: string;
  data: string;
  width: number;
  height: number;
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
  color?: string;
  arrowDirection?: EdgeArrowDirection;
}

export interface SubgraphData {
  id: string;
  label: string;
  x: number;
  y: number;
  width: number;
  height: number;
  labelX: number;
  labelY: number;
  depth: number;
  order: number;
  parentId?: string;
}

export interface DiagramData {
  sourcePath: string;
  background: string;
  autoSize: Size;
  renderSize: Size;
  nodes: NodeData[];
  edges: EdgeData[];
  subgraphs?: SubgraphData[];
  source: string;
}

export interface LayoutUpdate {
  nodes?: Record<string, Point | null>;
  edges?: Record<string, { points?: Point[] | null }>;
}

export interface NodeStyleUpdate {
  fill?: string | null;
  stroke?: string | null;
  text?: string | null;
}

export interface EdgeStyleUpdate {
  line?: EdgeKind | null;
  color?: string | null;
  arrow?: EdgeArrowDirection | null;
}

export interface StyleUpdate {
  nodeStyles?: Record<string, NodeStyleUpdate | null | undefined>;
  edgeStyles?: Record<string, EdgeStyleUpdate | null | undefined>;
}
