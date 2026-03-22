'use client';

import type { CodeMapMapping, DiagramData, LayoutUpdate, Point } from "../lib/types";

export interface DiagramCanvasProps {
  diagram: DiagramData;
  onNodeMove: (id: string, position: Point | null) => void;
  onLayoutUpdate?: (update: LayoutUpdate) => void;
  onEdgeMove: (id: string, points: Point[] | null) => void;
  selectedNodeId: string | null;
  selectedEdgeId: string | null;
  onSelectNode: (id: string | null) => void;
  onSelectEdge: (id: string | null) => void;
  onDragStateChange?: (dragging: boolean) => void;
  onDeleteNode: (id: string) => Promise<void> | void;
  onDeleteEdge: (id: string) => Promise<void> | void;
  codeMapMapping?: CodeMapMapping | null;
}
