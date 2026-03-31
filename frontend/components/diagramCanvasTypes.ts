'use client';

import type { CodeMapMapping, DiagramData, LayoutUpdate, NodeShape, Point, EdgeKind } from "../lib/types";

export type CanvasToolMode = "select" | "add-node" | "connect";

export interface CanvasContextMenuRequest {
  kind: "canvas" | "node" | "edge";
  clientX: number;
  clientY: number;
  point: Point | null;
  nodeId?: string;
  edgeId?: string;
}

export interface DiagramCanvasProps {
  diagram: DiagramData;
  onNodeMove: (id: string, position: Point | null) => void;
  onLayoutUpdate?: (update: LayoutUpdate) => void;
  onEdgeMove: (id: string, points: Point[] | null) => void;
  onSvgMarkupChange?: (markup: string) => void;
  selectedNodeId: string | null;
  selectedNodeIds: string[];
  selectedEdgeId: string | null;
  onSelectNode: (id: string | null) => void;
  onSelectNodes: (ids: string[]) => void;
  onSelectEdge: (id: string | null) => void;
  onDragStateChange?: (dragging: boolean) => void;
  onDeleteNode: (id: string) => Promise<void> | void;
  onDeleteEdge: (id: string) => Promise<void> | void;
  codeMapMapping?: CodeMapMapping | null;
  activeTool: CanvasToolMode;
  connectStartNodeId: string | null;
  addNodeSourceId: string | null;
  toolNodeShape: NodeShape;
  toolEdgeKind: EdgeKind;
  toolEdgeDirected: boolean;
  onCanvasAddNode: (point: Point) => void;
  onAddNodeSourceSelect: (id: string | null) => void;
  onConnectNodeClick: (id: string) => void;
  onContextMenuRequest?: (request: CanvasContextMenuRequest) => void;
  onViewportControlsChange?: (controls: {
    zoomIn: () => void;
    zoomOut: () => void;
    resetZoom: () => void;
    getScale: () => number;
  } | null) => void;
}
