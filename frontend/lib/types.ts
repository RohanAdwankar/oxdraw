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
export type EdgeKind = "solid" | "dashed" | "thick" | "invisible";
export type EdgeArrowDirection = "forward" | "backward" | "both" | "none";
export type EdgeEndpointMarker =
  | "none"
  | "arrow"
  | "triangle"
  | "diamond"
  | "diamond-open";

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
  labelFillColor?: string;
  imageFillColor?: string;
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
  padding: number;
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
  markerStart?: EdgeEndpointMarker;
  markerEnd?: EdgeEndpointMarker;
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

export interface GanttTaskData {
  id: string;
  label: string;
  sectionIndex: number;
  rowIndex: number;
  startDay: number;
  endDay: number;
  milestone: boolean;
}

export interface GanttStyleData {
  rowFillEven: string;
  rowFillOdd: string;
  taskFill: string;
  milestoneFill: string;
  taskText: string;
  milestoneText: string;
}

export interface GanttData {
  dateFormat: string;
  title?: string;
  minDay: number;
  maxDay: number;
  sectionLabelWidth: number;
  timelineWidth: number;
  topMargin: number;
  rowHeight: number;
  barHeight: number;
  rightPadding: number;
  bottomMargin: number;
  sections: string[];
  tasks: GanttTaskData[];
  style: GanttStyleData;
}

export interface DiagramData {
  sourcePath: string;
  kind: "flowchart" | "gantt";
  background: string;
  autoSize: Size;
  renderSize: Size;
  nodes: NodeData[];
  edges: EdgeData[];
  subgraphs?: SubgraphData[];
  gantt?: GanttData;
  source: string;
}

export interface LayoutUpdate {
  nodes?: Record<string, Point | null>;
  edges?: Record<string, { points?: Point[] | null }>;
  ganttTasks?: Record<string, { startDay?: number; endDay?: number } | null>;
}

export interface NodeStyleUpdate {
  fill?: string | null;
  stroke?: string | null;
  text?: string | null;
  labelFill?: string | null;
  imageFill?: string | null;
}

export interface EdgeStyleUpdate {
  line?: EdgeKind | null;
  color?: string | null;
  arrow?: EdgeArrowDirection | null;
}

export interface StyleUpdate {
  nodeStyles?: Record<string, NodeStyleUpdate | null | undefined>;
  edgeStyles?: Record<string, EdgeStyleUpdate | null | undefined>;
  ganttStyle?: {
    rowFillEven?: string | null;
    rowFillOdd?: string | null;
    taskFill?: string | null;
    milestoneFill?: string | null;
    taskText?: string | null;
    milestoneText?: string | null;
  };
}

export interface SearchResult {
  file: string;
  line: number;
  content: string;
}

export interface CodeLocation {
  file: string;
  start_line?: number;
  end_line?: number;
  symbol?: string;
}

export interface CodeMapMapping {
  nodes: Record<string, CodeLocation>;
}
