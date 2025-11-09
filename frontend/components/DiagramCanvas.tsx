'use client';

import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type CSSProperties,
} from "react";
import type {
  MouseEvent as ReactMouseEvent,
  PointerEvent as ReactPointerEvent,
} from "react";
import {
  DiagramData,
  EdgeArrowDirection,
  EdgeData,
  EdgeKind,
  LayoutUpdate,
  Size,
  Point,
} from "../lib/types";

const DEFAULT_NODE_WIDTH = 140;
const DEFAULT_NODE_HEIGHT = 60;
const NODE_LABEL_HEIGHT = 28;
const NODE_TEXT_LINE_HEIGHT = 16;
const LAYOUT_MARGIN = 80;
const HANDLE_RADIUS = 6;
const EPSILON = 0.5;
const GRID_SIZE = 10;
const ALIGN_THRESHOLD = 8;
const BOUNDS_SMOOTHING = 0.18;
const BOUNDS_EPSILON = 0.5;
const EDGE_LABEL_MIN_WIDTH = 36;
const EDGE_LABEL_MIN_HEIGHT = 28;
const EDGE_LABEL_LINE_HEIGHT = 16;
const EDGE_LABEL_FONT_SIZE = 13;
const EDGE_LABEL_HORIZONTAL_PADDING = 16;
const EDGE_LABEL_VERTICAL_PADDING = 12;
const EDGE_LABEL_VERTICAL_OFFSET = 10;
const EDGE_LABEL_BORDER_RADIUS = 6;
const EDGE_LABEL_BACKGROUND = "white";
const EDGE_LABEL_BACKGROUND_OPACITY = 0.96;

const svgSafeId = (prefix: string, id: string): string =>
  `${prefix}${id.replace(/[^a-zA-Z0-9_:-]/g, "_")}`;

const SHAPE_COLORS: Record<DiagramData["nodes"][number]["shape"], string> = {
  rectangle: "#FDE68A",
  stadium: "#C4F1F9",
  circle: "#E9D8FD",
  "double-circle": "#BFDBFE",
  diamond: "#FBCFE8",
  subroutine: "#FED7AA",
  cylinder: "#BBF7D0",
  hexagon: "#FCA5A5",
  parallelogram: "#C7D2FE",
  "parallelogram-alt": "#A5F3FC",
  trapezoid: "#FCE7F3",
  "trapezoid-alt": "#FCD5CE",
  asymmetric: "#F5D0FE",
};

const polygonPoints = (points: [number, number][]) =>
  points.map(([x, y]) => `${x.toFixed(1)},${y.toFixed(1)}`).join(" ");

const DEFAULT_NODE_STROKE = "#2d3748";
const DEFAULT_NODE_TEXT = "#1a202c";
const DEFAULT_EDGE_COLOR = "#2d3748";
const SUBGRAPH_FILL = "#edf2f7";
const SUBGRAPH_STROKE = "#a0aec0";
const SUBGRAPH_LABEL_COLOR = "#2d3748";
const SUBGRAPH_BORDER_RADIUS = 16;
const SUBGRAPH_SEPARATION = 140;

interface DiagramCanvasProps {
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
}

interface NodeDragState {
  type: "node";
  id: string;
  offset: Point;
  current: Point;
  moved: boolean;
}

interface EdgeDragState {
  type: "edge";
  id: string;
  index: number;
  points: Point[];
  moved: boolean;
  hasOverride: boolean;
}

interface SubgraphDragState {
  type: "subgraph";
  id: string;
  offset: Point;
  origin: Point;
  members: string[];
  nodeOffsets: Record<string, Point>;
  subgraphIds: string[];
  edgeOverrides: Record<string, Point[]>;
  delta: Point;
  moved: boolean;
}

type DragState = NodeDragState | EdgeDragState | SubgraphDragState | null;

type DraftNodes = Record<string, Point>;
type DraftEdges = Record<string, Point[]>;
type DraftSubgraphs = Record<string, Point>;

interface NodeBox {
  left: number;
  right: number;
  centerX: number;
  top: number;
  bottom: number;
  centerY: number;
}

interface SubgraphRect {
  left: number;
  right: number;
  top: number;
  bottom: number;
  centerX: number;
  centerY: number;
  width: number;
  height: number;
}

interface VerticalGuide {
  axis: "vertical";
  x: number;
  y1: number;
  y2: number;
  kind: "edge" | "center";
  sourceId: string;
  targetId: string;
}

interface HorizontalGuide {
  axis: "horizontal";
  y: number;
  x1: number;
  x2: number;
  kind: "edge" | "center";
  sourceId: string;
  targetId: string;
}

interface AlignmentGuides {
  vertical?: VerticalGuide;
  horizontal?: HorizontalGuide;
}

interface AlignmentResult {
  position: Point;
  guides: AlignmentGuides;
  appliedX: boolean;
  appliedY: boolean;
}

const EMPTY_GUIDES: AlignmentGuides = {};

interface EdgeView {
  edge: EdgeData;
  route: Point[];
  handlePoints: Point[];
  hasOverride: boolean;
  color: string;
  arrowDirection: EdgeArrowDirection;
  labelHandleIndex: number | null;
  labelPoint: Point;
}

interface ContextMenuState {
  visible: boolean;
  x: number;
  y: number;
  target: { type: "node" | "edge"; id: string } | null;
}

function midpoint(a: Point, b: Point): Point {
  return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
}

function isClose(a: Point, b: Point): boolean {
  return Math.abs(a.x - b.x) < EPSILON && Math.abs(a.y - b.y) < EPSILON;
}

function centroid(points: readonly Point[]): Point {
  if (points.length === 0) {
    return { x: 0, y: 0 };
  }
  let sumX = 0;
  let sumY = 0;
  for (const point of points) {
    sumX += point.x;
    sumY += point.y;
  }
  return { x: sumX / points.length, y: sumY / points.length };
}

function distanceToSegment(point: Point, start: Point, end: Point): number {
  const vx = end.x - start.x;
  const vy = end.y - start.y;
  const wx = point.x - start.x;
  const wy = point.y - start.y;
  const lengthSquared = vx * vx + vy * vy;

  if (lengthSquared === 0) {
    return Math.hypot(point.x - start.x, point.y - start.y);
  }

  let t = (wx * vx + wy * vy) / lengthSquared;
  if (t < 0) {
    t = 0;
  } else if (t > 1) {
    t = 1;
  }

  const projectionX = start.x + t * vx;
  const projectionY = start.y + t * vy;
  return Math.hypot(point.x - projectionX, point.y - projectionY);
}

function normalizeLabelLines(label: string): string[] {
  return label
    .split("\n")
    .map((line) => (line.length === 0 ? "\u00A0" : line));
}

function measureLabelBox(lines: string[]): Size {
  let maxChars = 0;
  for (const line of lines) {
    maxChars = Math.max(maxChars, line.length);
  }

  const width = Math.max(
    EDGE_LABEL_MIN_WIDTH,
    7.4 * maxChars + EDGE_LABEL_HORIZONTAL_PADDING
  );
  const height = Math.max(
    EDGE_LABEL_MIN_HEIGHT,
    EDGE_LABEL_LINE_HEIGHT * lines.length + EDGE_LABEL_VERTICAL_PADDING
  );

  return { width, height };
}

function interiorPoints(route: readonly Point[]): Point[] {
  if (route.length <= 2) {
    return [];
  }
  return route.slice(1, route.length - 1).map((point) => ({ ...point }));
}

function labelCenterForRoute(route: readonly Point[]): Point {
  if (route.length === 0) {
    return { x: 0, y: -EDGE_LABEL_VERTICAL_OFFSET };
  }

  const fallback = centroid(route);
  if (route.length <= 2) {
    return { x: fallback.x, y: fallback.y - EDGE_LABEL_VERTICAL_OFFSET };
  }

  const candidates = route.slice(1, route.length - 1);
  if (candidates.length === 0) {
    return { x: fallback.x, y: fallback.y - EDGE_LABEL_VERTICAL_OFFSET };
  }

  if (candidates.length === 1) {
    const point = candidates[0];
    return { x: point.x, y: point.y };
  }

  let best = candidates[0];
  let bestDistance = Number.POSITIVE_INFINITY;
  for (const point of candidates) {
    const distance = Math.hypot(point.x - fallback.x, point.y - fallback.y);
    if (distance < bestDistance) {
      bestDistance = distance;
      best = point;
    }
  }

  return { x: best.x, y: best.y };
}

function defaultHandleForRoute(
  route: readonly Point[],
  start: Point,
  end: Point
): Point {
  const interior = interiorPoints(route);
  if (interior.length > 0) {
    const index = Math.floor(interior.length / 2);
    return { ...interior[index] };
  }
  return midpoint(start, end);
}

function snapToGrid(value: number): number {
  if (GRID_SIZE <= 0) {
    return value;
  }
  return Math.round(value / GRID_SIZE) * GRID_SIZE;
}

function createNodeBox(position: Point, width: number, height: number): NodeBox {
  const halfWidth = width / 2;
  const halfHeight = height / 2;
  return {
    left: position.x - halfWidth,
    right: position.x + halfWidth,
    centerX: position.x,
    top: position.y - halfHeight,
    bottom: position.y + halfHeight,
    centerY: position.y,
  };
}

function createSubgraphRect(
  subgraph: NonNullable<DiagramData["subgraphs"]>[number],
  offset?: Point
): SubgraphRect {
  const dx = offset?.x ?? 0;
  const dy = offset?.y ?? 0;
  const left = subgraph.x + dx;
  const top = subgraph.y + dy;
  const width = subgraph.width;
  const height = subgraph.height;
  return {
    left,
    right: left + width,
    top,
    bottom: top + height,
    centerX: left + width / 2,
    centerY: top + height / 2,
    width,
    height,
  };
}

function computeSubgraphSeparationShift(
  moving: SubgraphRect,
  stationary: SubgraphRect,
  margin: number
): Point | null {
  const expanded = {
    left: stationary.left - margin,
    right: stationary.right + margin,
    top: stationary.top - margin,
    bottom: stationary.bottom + margin,
  };

  const overlapX = Math.min(moving.right, expanded.right) - Math.max(moving.left, expanded.left);
  const overlapY = Math.min(moving.bottom, expanded.bottom) - Math.max(moving.top, expanded.top);

  if (overlapX <= 0 || overlapY <= 0) {
    return null;
  }

  const horizontalShift = (() => {
    if (moving.centerX >= stationary.centerX) {
      const target = expanded.right;
      const shift = target - moving.left;
      return shift + (shift >= 0 ? EPSILON : -EPSILON);
    }
    const target = expanded.left;
    const shift = target - moving.right;
    return shift + (shift >= 0 ? EPSILON : -EPSILON);
  })();

  const verticalShift = (() => {
    if (moving.centerY >= stationary.centerY) {
      const target = expanded.bottom;
      const shift = target - moving.top;
      return shift + (shift >= 0 ? EPSILON : -EPSILON);
    }
    const target = expanded.top;
    const shift = target - moving.bottom;
    return shift + (shift >= 0 ? EPSILON : -EPSILON);
  })();

  const absHorizontal = Math.abs(horizontalShift);
  const absVertical = Math.abs(verticalShift);

  if (absHorizontal <= absVertical && absHorizontal > 0) {
    return { x: horizontalShift, y: 0 };
  }

  if (absVertical > 0) {
    return { x: 0, y: verticalShift };
  }

  return null;
}

function resolveNodeDimensions(id: string, dimensions: Map<string, Size>): Size {
  const dims = dimensions.get(id);
  if (dims) {
    return dims;
  }
  if (id.startsWith("edge:")) {
    return { width: 0, height: 0 };
  }
  return { width: DEFAULT_NODE_WIDTH, height: DEFAULT_NODE_HEIGHT };
}

function computeNodeAlignment(
  nodeId: string,
  proposed: Point,
  nodes: readonly [string, Point][],
  threshold: number,
  dimensions: Map<string, Size>
): AlignmentResult {
  const movingDimensions = resolveNodeDimensions(nodeId, dimensions);
  const movingBox = createNodeBox(proposed, movingDimensions.width, movingDimensions.height);
  let bestVertical: {
    diff: number;
    value: number;
    guide: VerticalGuide;
  } | null = null;
  let bestHorizontal: {
    diff: number;
    value: number;
    guide: HorizontalGuide;
  } | null = null;

  for (const [otherId, point] of nodes) {
    if (otherId === nodeId) {
      continue;
    }
  const otherDimensions = resolveNodeDimensions(otherId, dimensions);
  const otherBox = createNodeBox(point, otherDimensions.width, otherDimensions.height);

    const verticalCandidates = [
      {
        diff: otherBox.left - movingBox.left,
        value: () => proposed.x + (otherBox.left - movingBox.left),
        kind: "edge" as const,
        line: otherBox.left,
      },
      {
        diff: otherBox.right - movingBox.left,
        value: () => proposed.x + (otherBox.right - movingBox.left),
        kind: "edge" as const,
        line: otherBox.right,
      },
      {
        diff: otherBox.left - movingBox.right,
        value: () => proposed.x + (otherBox.left - movingBox.right),
        kind: "edge" as const,
        line: otherBox.left,
      },
      {
        diff: otherBox.right - movingBox.right,
        value: () => proposed.x + (otherBox.right - movingBox.right),
        kind: "edge" as const,
        line: otherBox.right,
      },
      {
        diff: otherBox.centerX - movingBox.centerX,
        value: () => proposed.x + (otherBox.centerX - movingBox.centerX),
        kind: "center" as const,
        line: otherBox.centerX,
      },
    ];

    for (const candidate of verticalCandidates) {
      const absDiff = Math.abs(candidate.diff);
      if (absDiff > threshold) {
        continue;
      }
      if (bestVertical && Math.abs(bestVertical.diff) <= absDiff) {
        continue;
      }
      const alignedX = candidate.value();
      const alignedBox = createNodeBox(
        { x: alignedX, y: proposed.y },
        movingDimensions.width,
        movingDimensions.height
      );
      bestVertical = {
        diff: candidate.diff,
        value: alignedX,
        guide: {
          axis: "vertical",
          x: candidate.kind === "center" ? alignedBox.centerX : candidate.line,
          y1: Math.min(alignedBox.top, otherBox.top),
          y2: Math.max(alignedBox.bottom, otherBox.bottom),
          kind: candidate.kind,
          sourceId: nodeId,
          targetId: otherId,
        },
      };
    }

    const horizontalCandidates = [
      {
        diff: otherBox.top - movingBox.top,
        value: () => proposed.y + (otherBox.top - movingBox.top),
        kind: "edge" as const,
        line: otherBox.top,
      },
      {
        diff: otherBox.bottom - movingBox.top,
        value: () => proposed.y + (otherBox.bottom - movingBox.top),
        kind: "edge" as const,
        line: otherBox.bottom,
      },
      {
        diff: otherBox.top - movingBox.bottom,
        value: () => proposed.y + (otherBox.top - movingBox.bottom),
        kind: "edge" as const,
        line: otherBox.top,
      },
      {
        diff: otherBox.bottom - movingBox.bottom,
        value: () => proposed.y + (otherBox.bottom - movingBox.bottom),
        kind: "edge" as const,
        line: otherBox.bottom,
      },
      {
        diff: otherBox.centerY - movingBox.centerY,
        value: () => proposed.y + (otherBox.centerY - movingBox.centerY),
        kind: "center" as const,
        line: otherBox.centerY,
      },
    ];

    for (const candidate of horizontalCandidates) {
      const absDiff = Math.abs(candidate.diff);
      if (absDiff > threshold) {
        continue;
      }
      if (bestHorizontal && Math.abs(bestHorizontal.diff) <= absDiff) {
        continue;
      }
      const alignedY = candidate.value();
      const alignedBox = createNodeBox(
        { x: proposed.x, y: alignedY },
        movingDimensions.width,
        movingDimensions.height
      );
      bestHorizontal = {
        diff: candidate.diff,
        value: alignedY,
        guide: {
          axis: "horizontal",
          y: candidate.kind === "center" ? alignedBox.centerY : candidate.line,
          x1: Math.min(alignedBox.left, otherBox.left),
          x2: Math.max(alignedBox.right, otherBox.right),
          kind: candidate.kind,
          sourceId: nodeId,
          targetId: otherId,
        },
      };
    }
  }

  const guides: AlignmentGuides = {};
  let appliedX = false;
  let appliedY = false;

  let finalX = proposed.x;
  if (bestVertical) {
    finalX = bestVertical.value;
    guides.vertical = bestVertical.guide;
    appliedX = true;
  }

  let finalY = proposed.y;
  if (bestHorizontal) {
    finalY = bestHorizontal.value;
    guides.horizontal = bestHorizontal.guide;
    appliedY = true;
  }

  if (!guides.vertical) {
    delete guides.vertical;
  }
  if (!guides.horizontal) {
    delete guides.horizontal;
  }

  const finalPosition = { x: finalX, y: finalY };
  const finalBox = createNodeBox(finalPosition, movingDimensions.width, movingDimensions.height);

  const verticalGuide = guides.vertical;
  if (verticalGuide) {
    const targetPoint = nodes.find((entry) => entry[0] === verticalGuide.targetId)?.[1];
    if (targetPoint) {
      const targetDimensions = resolveNodeDimensions(verticalGuide.targetId, dimensions);
      const targetBox = createNodeBox(
        targetPoint,
        targetDimensions.width,
        targetDimensions.height
      );
      guides.vertical = {
        ...verticalGuide,
        x:
          verticalGuide.kind === "center"
            ? finalBox.centerX
            : verticalGuide.x,
        y1: Math.min(finalBox.top, targetBox.top),
        y2: Math.max(finalBox.bottom, targetBox.bottom),
      };
    }
  }

  const horizontalGuide = guides.horizontal;
  if (horizontalGuide) {
    const targetPoint = nodes.find((entry) => entry[0] === horizontalGuide.targetId)?.[1];
    if (targetPoint) {
      const targetDimensions = resolveNodeDimensions(horizontalGuide.targetId, dimensions);
      const targetBox = createNodeBox(
        targetPoint,
        targetDimensions.width,
        targetDimensions.height
      );
      guides.horizontal = {
        ...horizontalGuide,
        y:
          horizontalGuide.kind === "center"
            ? finalBox.centerY
            : horizontalGuide.y,
        x1: Math.min(finalBox.left, targetBox.left),
        x2: Math.max(finalBox.right, targetBox.right),
      };
    }
  }

  const normalizedGuides = guides.vertical || guides.horizontal ? guides : EMPTY_GUIDES;

  return {
    position: finalPosition,
    guides: normalizedGuides,
    appliedX,
    appliedY,
  };
}

function guidesEqual(a: AlignmentGuides, b: AlignmentGuides): boolean {
  const aVertical = a.vertical;
  const bVertical = b.vertical;
  if (!!aVertical !== !!bVertical) {
    return false;
  }
  if (
    aVertical &&
    bVertical &&
    (aVertical.x !== bVertical.x ||
      aVertical.y1 !== bVertical.y1 ||
      aVertical.y2 !== bVertical.y2 ||
      aVertical.kind !== bVertical.kind ||
      aVertical.sourceId !== bVertical.sourceId ||
      aVertical.targetId !== bVertical.targetId)
  ) {
    return false;
  }

  const aHorizontal = a.horizontal;
  const bHorizontal = b.horizontal;
  if (!!aHorizontal !== !!bHorizontal) {
    return false;
  }
  if (
    aHorizontal &&
    bHorizontal &&
    (aHorizontal.y !== bHorizontal.y ||
      aHorizontal.x1 !== bHorizontal.x1 ||
      aHorizontal.x2 !== bHorizontal.x2 ||
      aHorizontal.kind !== bHorizontal.kind ||
      aHorizontal.sourceId !== bHorizontal.sourceId ||
      aHorizontal.targetId !== bHorizontal.targetId)
  ) {
    return false;
  }

  return true;
}

export default function DiagramCanvas({
  diagram,
  onNodeMove,
  onLayoutUpdate,
  onEdgeMove,
  selectedNodeId,
  selectedEdgeId,
  onSelectNode,
  onSelectEdge,
  onDragStateChange,
  onDeleteNode,
  onDeleteEdge,
}: DiagramCanvasProps) {
  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const svgRef = useRef<SVGSVGElement | null>(null);
  const [dragState, setDragState] = useState<DragState>(null);
  const [draftNodes, setDraftNodes] = useState<DraftNodes>({});
  const [draftEdges, setDraftEdges] = useState<DraftEdges>({});
  const [draftSubgraphs, setDraftSubgraphs] = useState<DraftSubgraphs>({});
  const [alignmentGuides, setAlignmentGuides] = useState<AlignmentGuides>({});
  const [contextMenu, setContextMenu] = useState<ContextMenuState>({
    visible: false,
    x: 0,
    y: 0,
    target: null,
  });

  const closeContextMenu = useCallback(() => {
    setContextMenu((prev) =>
      prev.visible ? { visible: false, x: 0, y: 0, target: null } : prev
    );
  }, []);

  const openContextMenu = useCallback(
    (event: ReactMouseEvent, target: { type: "node" | "edge"; id: string }) => {
      event.preventDefault();
      const wrapper = wrapperRef.current;
      if (!wrapper) {
        return;
      }
      const rect = wrapper.getBoundingClientRect();
      setContextMenu({
        visible: true,
        x: event.clientX - rect.left,
        y: event.clientY - rect.top,
        target,
      });
    },
    []
  );

  useEffect(() => {
    if (!contextMenu.visible) {
      return;
    }

    const handlePointerDown = (event: PointerEvent) => {
      const wrapper = wrapperRef.current;
      if (!wrapper) {
        return;
      }
      if (!wrapper.contains(event.target as Node)) {
        closeContextMenu();
      }
    };

    const handleEscape = (event: KeyboardEvent) => {
      if (event.key === "Escape") {
        closeContextMenu();
      }
    };

    window.addEventListener("pointerdown", handlePointerDown);
    window.addEventListener("keydown", handleEscape);

    return () => {
      window.removeEventListener("pointerdown", handlePointerDown);
      window.removeEventListener("keydown", handleEscape);
    };
  }, [contextMenu.visible, closeContextMenu]);

  useEffect(() => {
    closeContextMenu();
  }, [diagram, closeContextMenu]);

  const handleContextMenuDelete = useCallback(() => {
    setContextMenu((prev) => {
      if (prev.target) {
        if (prev.target.type === "node") {
          void onDeleteNode(prev.target.id);
        } else {
          void onDeleteEdge(prev.target.id);
        }
      }
      return { visible: false, x: 0, y: 0, target: null };
    });
  }, [onDeleteEdge, onDeleteNode]);

  const finalPositions = useMemo(() => {
    const map = new Map<string, Point>();
    for (const node of diagram.nodes) {
      const override = draftNodes[node.id] ?? node.overridePosition ?? node.renderedPosition;
      map.set(node.id, override);
    }
    return map;
  }, [diagram.nodes, draftNodes]);

  const edges = useMemo<EdgeView[]>(() => {
    return diagram.edges
      .map((edge) => {
        const from = finalPositions.get(edge.from);
        const to = finalPositions.get(edge.to);
        if (!from || !to) {
          return null;
        }

        const draftOverride = draftEdges[edge.id];
        const hasDraftOverride = draftOverride !== undefined;
        const baseOverrides = draftOverride ?? edge.overridePoints ?? [];
        const overridePoints = baseOverrides.map((point) => ({ x: point.x, y: point.y }));
        const hasOverride = overridePoints.length > 0;

        const renderedRoute = edge.renderedPoints.length >= 2
          ? edge.renderedPoints.map((point) => ({ x: point.x, y: point.y }))
          : [
            { x: from.x, y: from.y },
            { x: to.x, y: to.y },
          ];

        const route = hasDraftOverride
          ? [
            { x: from.x, y: from.y },
            ...overridePoints,
            { x: to.x, y: to.y },
          ]
          : renderedRoute;

        const handlePoints = hasOverride
          ? overridePoints
          : [defaultHandleForRoute(renderedRoute, from, to)];

        let labelHandleIndex: number | null = null;
        if (edge.label && hasOverride && handlePoints.length > 0) {
          if (handlePoints.length === 1) {
            labelHandleIndex = 0;
          } else {
            const routeCentroid = centroid(route);
            let bestIndex = 0;
            let bestDistance = Number.POSITIVE_INFINITY;
            handlePoints.forEach((point, idx) => {
              const distance = Math.hypot(point.x - routeCentroid.x, point.y - routeCentroid.y);
              if (distance < bestDistance) {
                bestDistance = distance;
                bestIndex = idx;
              }
            });
            labelHandleIndex = bestIndex;
          }
        }

        const labelPoint =
          labelHandleIndex !== null ? { ...handlePoints[labelHandleIndex] } : labelCenterForRoute(route);

        const color = edge.color ?? DEFAULT_EDGE_COLOR;
        const arrowDirection = edge.arrowDirection ?? "forward";

        return {
          edge,
          route,
          handlePoints,
          hasOverride,
          color,
          arrowDirection,
          labelHandleIndex,
          labelPoint,
        };
      })
      .filter((value): value is EdgeView => value !== null);
  }, [diagram.edges, draftEdges, finalPositions]);

  const subgraphViews = useMemo(() => {
    const items = diagram.subgraphs ?? [];
    return [...items].sort((a, b) => {
      if (a.depth !== b.depth) {
        return a.depth - b.depth;
      }
      if (a.order !== b.order) {
        return a.order - b.order;
      }
      return a.id.localeCompare(b.id);
    });
  }, [diagram.subgraphs]);

  const subgraphById = useMemo(() => {
    const map = new Map<string, (typeof subgraphViews)[number]>();
    for (const subgraph of subgraphViews) {
      map.set(subgraph.id, subgraph);
    }
    return map;
  }, [subgraphViews]);

  const nodeDimensions = useMemo(() => {
    const map = new Map<string, Size>();
    for (const node of diagram.nodes) {
      const width = Number.isFinite(node.width) && node.width > 0 ? node.width : DEFAULT_NODE_WIDTH;
      const height = Number.isFinite(node.height) && node.height > 0 ? node.height : DEFAULT_NODE_HEIGHT;
      map.set(node.id, { width, height });
    }
    return map;
  }, [diagram.nodes]);

  const fitBounds = useMemo(() => {
    // Zoom-to-fit: include all nodes, edge control points, and label backgrounds.
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;

    const extend = (point: Point, halfWidth = 0, halfHeight = 0) => {
      minX = Math.min(minX, point.x - halfWidth);
      maxX = Math.max(maxX, point.x + halfWidth);
      minY = Math.min(minY, point.y - halfHeight);
      maxY = Math.max(maxY, point.y + halfHeight);
    };

    for (const [id, position] of finalPositions.entries()) {
      const dims = nodeDimensions.get(id) ?? {
        width: DEFAULT_NODE_WIDTH,
        height: DEFAULT_NODE_HEIGHT,
      };
      extend(position, dims.width / 2, dims.height / 2);
    }

    for (const view of edges) {
      for (const point of view.route) {
        extend(point);
      }
      for (const point of view.handlePoints) {
        extend(point);
      }
      if (view.edge.label) {
        const labelLines = normalizeLabelLines(view.edge.label);
        if (labelLines.length > 0) {
          const labelSize = measureLabelBox(labelLines);
          extend(view.labelPoint, labelSize.width / 2, labelSize.height / 2);
        }
      }
    }

    for (const subgraph of subgraphViews) {
      const offset = draftSubgraphs[subgraph.id];
      const left = subgraph.x + (offset?.x ?? 0);
      const top = subgraph.y + (offset?.y ?? 0);
      const center = {
        x: left + subgraph.width / 2,
        y: top + subgraph.height / 2,
      };
      extend(center, subgraph.width / 2, subgraph.height / 2);
      const labelPoint = {
        x: subgraph.labelX + (offset?.x ?? 0),
        y: subgraph.labelY + (offset?.y ?? 0),
      };
      extend(labelPoint, 0, 0);
    }

    if (!Number.isFinite(minX)) {
      minX = -DEFAULT_NODE_WIDTH / 2;
      maxX = DEFAULT_NODE_WIDTH / 2;
      minY = -DEFAULT_NODE_HEIGHT / 2;
      maxY = DEFAULT_NODE_HEIGHT / 2;
    }

    const width = Math.max(maxX - minX, DEFAULT_NODE_WIDTH) + LAYOUT_MARGIN * 2;
    const height = Math.max(maxY - minY, DEFAULT_NODE_HEIGHT) + LAYOUT_MARGIN * 2;
    const offsetX = LAYOUT_MARGIN - minX;
    const offsetY = LAYOUT_MARGIN - minY;

    return { width, height, offsetX, offsetY };
  }, [draftSubgraphs, edges, finalPositions, subgraphViews, nodeDimensions]);

  const [bounds, setBounds] = useState(() => fitBounds);

  useEffect(() => {
    let frame: number | null = null;

    const animate = () => {
      let finished = false;
      setBounds((prev) => {
        const lerp = (a: number, b: number) => a + (b - a) * BOUNDS_SMOOTHING;
        const next = {
          width: lerp(prev.width, fitBounds.width),
          height: lerp(prev.height, fitBounds.height),
          offsetX: lerp(prev.offsetX, fitBounds.offsetX),
          offsetY: lerp(prev.offsetY, fitBounds.offsetY),
        };

        const closeEnough =
          Math.abs(next.width - fitBounds.width) < BOUNDS_EPSILON &&
          Math.abs(next.height - fitBounds.height) < BOUNDS_EPSILON &&
          Math.abs(next.offsetX - fitBounds.offsetX) < BOUNDS_EPSILON &&
          Math.abs(next.offsetY - fitBounds.offsetY) < BOUNDS_EPSILON;

        if (closeEnough) {
          finished = true;
          return fitBounds;
        }

        return next;
      });

      if (!finished) {
        frame = requestAnimationFrame(animate);
      }
    };

    frame = requestAnimationFrame(animate);
    return () => {
      if (frame !== null) {
        cancelAnimationFrame(frame);
      }
    };
  }, [fitBounds]);

  const nodeEntries = useMemo<[string, Point][]>(() => {
    return Array.from(finalPositions.entries());
  }, [finalPositions]);

  const alignmentEntries = useMemo<[string, Point][]>(() => {
    const combined: [string, Point][] = [...nodeEntries];
    for (const view of edges) {
      view.handlePoints.forEach((point, index) => {
        combined.push([`edge:${view.edge.id}:handle:${index}`, point]);
      });
    }
    return combined;
  }, [edges, nodeEntries]);

  const nodesBySubgraph = useMemo(() => {
    const map = new Map<string, Set<string>>();
    for (const node of diagram.nodes) {
      const memberships = node.membership ?? [];
      for (const subgraphId of memberships) {
        let bucket = map.get(subgraphId);
        if (!bucket) {
          bucket = new Set<string>();
          map.set(subgraphId, bucket);
        }
        bucket.add(node.id);
      }
    }
    return map;
  }, [diagram.nodes]);

  const subgraphChildren = useMemo(() => {
    const map = new Map<string, string[]>();
    for (const subgraph of diagram.subgraphs ?? []) {
      if (!subgraph.parentId) {
        continue;
      }
      const existing = map.get(subgraph.parentId);
      if (existing) {
        existing.push(subgraph.id);
      } else {
        map.set(subgraph.parentId, [subgraph.id]);
      }
    }
    return map;
  }, [diagram.subgraphs]);

  const gatherSubgraphDescendants = useCallback(
    (rootId: string) => {
      const result: string[] = [];
      const stack: string[] = [rootId];
      while (stack.length > 0) {
        const current = stack.pop();
        if (!current) {
          continue;
        }
        result.push(current);
        const children = subgraphChildren.get(current);
        if (children) {
          for (const child of children) {
            stack.push(child);
          }
        }
      }
      return result;
    },
    [subgraphChildren]
  );

  const resolveSubgraphDelta = useCallback(
    (drag: SubgraphDragState, proposed: Point): Point => {
      const root = subgraphById.get(drag.id);
      if (!root) {
        return { x: proposed.x, y: proposed.y };
      }

      const rootParent = root.parentId ?? null;
      const excluded = new Set<string>(drag.subgraphIds);
      const siblings = subgraphViews.filter((candidate) => {
        if (excluded.has(candidate.id)) {
          return false;
        }
        const candidateParent = candidate.parentId ?? null;
        return candidateParent === rootParent;
      });

      if (siblings.length === 0) {
        return { x: proposed.x, y: proposed.y };
      }

      let delta = { x: proposed.x, y: proposed.y };
      const maxIterations = 6;

      for (let iteration = 0; iteration < maxIterations; iteration += 1) {
        let adjusted = false;
        const movingRect = createSubgraphRect(root, delta);

        for (const candidate of siblings) {
          const candidateOffset = draftSubgraphs[candidate.id];
          const candidateRect = createSubgraphRect(candidate, candidateOffset);
          const shift = computeSubgraphSeparationShift(
            movingRect,
            candidateRect,
            SUBGRAPH_SEPARATION
          );
          if (!shift) {
            continue;
          }
          delta = { x: delta.x + shift.x, y: delta.y + shift.y };
          adjusted = true;
          break;
        }

        if (!adjusted) {
          break;
        }
      }

      return delta;
    },
    [draftSubgraphs, subgraphById, subgraphViews]
  );

  const toScreen = (point: Point) => ({
    x: point.x + bounds.offsetX,
    y: point.y + bounds.offsetY,
  });

  const verticalGuide = alignmentGuides.vertical;
  const horizontalGuide = alignmentGuides.horizontal;

  const getDiagramPointFromClient = (clientX: number, clientY: number): Point | null => {
    const svg = svgRef.current;
    if (!svg) {
      return null;
    }
    const point = svg.createSVGPoint();
    point.x = clientX;
    point.y = clientY;
    const ctm = svg.getScreenCTM();
    if (!ctm) {
      return null;
    }
    const transformed = point.matrixTransform(ctm.inverse());
    return {
      x: transformed.x - bounds.offsetX,
      y: transformed.y - bounds.offsetY,
    };
  };

  const clientToDiagram = (event: ReactPointerEvent): Point | null => {
    return getDiagramPointFromClient(event.clientX, event.clientY);
  };

  const handleCanvasPointerDown = (event: ReactPointerEvent<SVGSVGElement>) => {
    closeContextMenu();
    if (event.target === event.currentTarget) {
      onSelectNode(null);
      onSelectEdge(null);
    }
  };

  const handleCanvasContextMenu = (event: ReactMouseEvent<SVGSVGElement>) => {
    event.preventDefault();
    closeContextMenu();
  };

  const handleSubgraphPointerDown = (id: string, event: ReactPointerEvent<SVGGElement>) => {
    event.preventDefault();
    event.stopPropagation();
    closeContextMenu();
    const diagramPoint = clientToDiagram(event);
    if (!diagramPoint) {
      return;
    }

    const subgraph = subgraphViews.find((item) => item.id === id);
    if (!subgraph) {
      return;
    }

    const membership = nodesBySubgraph.get(id);
    if (!membership || membership.size === 0) {
      return;
    }

    const offsetEntry = draftSubgraphs[id];
    const currentTopLeft = {
      x: subgraph.x + (offsetEntry?.x ?? 0),
      y: subgraph.y + (offsetEntry?.y ?? 0),
    };

    const members = Array.from(membership);
    const initialNodePositions: Record<string, Point> = {};
    const nodeOffsets: Record<string, Point> = {};
    for (const nodeId of members) {
      const position = finalPositions.get(nodeId);
      if (position) {
        initialNodePositions[nodeId] = { ...position };
        nodeOffsets[nodeId] = {
          x: position.x - currentTopLeft.x,
          y: position.y - currentTopLeft.y,
        };
      }
    }

    if (Object.keys(initialNodePositions).length === 0) {
      return;
    }

    const offset = {
      x: diagramPoint.x - currentTopLeft.x,
      y: diagramPoint.y - currentTopLeft.y,
    };

    const subgraphIds = gatherSubgraphDescendants(id);

    const memberSet = new Set(members);
    const edgeOverrides: Record<string, Point[]> = {};
    for (const edge of diagram.edges) {
      const baseOverride = draftEdges[edge.id] ?? edge.overridePoints;
      if (!baseOverride || baseOverride.length === 0) {
        continue;
      }
      if (!memberSet.has(edge.from) && !memberSet.has(edge.to)) {
        continue;
      }
      edgeOverrides[edge.id] = baseOverride.map((point) => ({ x: point.x, y: point.y }));
    }

    onDragStateChange?.(true);
    setDragState({
      type: "subgraph",
      id,
      offset,
      origin: currentTopLeft,
      members,
      nodeOffsets,
      subgraphIds,
      edgeOverrides,
      delta: { x: 0, y: 0 },
      moved: false,
    });

    setDraftNodes((prev: DraftNodes) => {
      const next = { ...prev };
      for (const [nodeId, base] of Object.entries(initialNodePositions)) {
        next[nodeId] = base;
      }
      return next;
    });

    setDraftSubgraphs((prev: DraftSubgraphs) => {
      const next = { ...prev };
      for (const subgraphId of subgraphIds) {
        next[subgraphId] = prev[subgraphId] ?? { x: 0, y: 0 };
      }
      return next;
    });

    event.currentTarget.setPointerCapture(event.pointerId);
    onSelectNode(null);
    onSelectEdge(null);
  };

  const handleNodePointerDown = (id: string, event: ReactPointerEvent<SVGGElement>) => {
    event.preventDefault();
    event.stopPropagation();
    closeContextMenu();
    const diagramPoint = clientToDiagram(event);
    if (!diagramPoint) {
      return;
    }
    const current = finalPositions.get(id);
    if (!current) {
      return;
    }
    const offset = {
      x: diagramPoint.x - current.x,
      y: diagramPoint.y - current.y,
    };
    onDragStateChange?.(true);
    setDragState({ type: "node", id, offset, current, moved: false });
    setDraftNodes((prev: DraftNodes) => ({ ...prev, [id]: current }));
    event.currentTarget.setPointerCapture(event.pointerId);
    onSelectNode(id);
    onSelectEdge(null);
  };

  const handleNodeContextMenu = (id: string, event: ReactMouseEvent<SVGGElement>) => {
    openContextMenu(event, { type: "node", id });
    onSelectNode(id);
    onSelectEdge(null);
  };

  const handleHandlePointerDown = (
    edgeId: string,
    index: number,
    availablePoints: Point[],
    hasOverride: boolean,
    event: ReactPointerEvent<SVGElement>
  ) => {
    event.preventDefault();
    event.stopPropagation();
    closeContextMenu();
    const basePoints = hasOverride
      ? availablePoints.map((point: Point) => ({ ...point }))
      : [availablePoints[index] ?? availablePoints[0]];
    onDragStateChange?.(true);
    setDragState({
      type: "edge",
      id: edgeId,
      index: hasOverride ? index : 0,
      points: basePoints,
      moved: false,
      hasOverride,
    });
    setDraftEdges((prev: DraftEdges) => ({ ...prev, [edgeId]: basePoints }));
    event.currentTarget.setPointerCapture(event.pointerId);
    onSelectEdge(edgeId);
    onSelectNode(null);
  };

  const handleEdgePointerDown = (
    edgeId: string,
    event: ReactPointerEvent<SVGElement>
  ) => {
    event.stopPropagation();
    closeContextMenu();
    onSelectEdge(edgeId);
    onSelectNode(null);
  };

  const handleEdgeContextMenu = (edgeId: string, event: ReactMouseEvent<SVGElement>) => {
    openContextMenu(event, { type: "edge", id: edgeId });
    onSelectEdge(edgeId);
    onSelectNode(null);
  };

  const handleEdgeDoubleClick = (
    edgeId: string,
    handlePoints: Point[],
    pathPoints: Point[],
    event: ReactMouseEvent<Element>
  ) => {
    event.preventDefault();
    event.stopPropagation();

    const diagramPoint = getDiagramPointFromClient(event.clientX, event.clientY);
    if (!diagramPoint) {
      return;
    }

    const basePoints = handlePoints.map((point) => ({ ...point }));

    if (basePoints.some((point) => isClose(point, diagramPoint))) {
      return;
    }

    if (basePoints.length === 0) {
      basePoints.push(diagramPoint);
    } else {
      let bestSegment = 0;
      let bestDistance = Number.POSITIVE_INFINITY;
      for (let index = 0; index < pathPoints.length - 1; index += 1) {
        const distance = distanceToSegment(diagramPoint, pathPoints[index], pathPoints[index + 1]);
        if (distance < bestDistance) {
          bestDistance = distance;
          bestSegment = index;
        }
      }

      const insertIndex = Math.min(bestSegment, basePoints.length);
      basePoints.splice(insertIndex, 0, diagramPoint);
    }

    const nextPoints = basePoints.map((point) => ({ ...point }));
    setDraftEdges((prev: DraftEdges) => ({ ...prev, [edgeId]: nextPoints }));
    onEdgeMove(edgeId, nextPoints);
    onSelectEdge(edgeId);
    onSelectNode(null);
  };

  const handlePointerMove = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (!dragState) {
      return;
    }
    const diagramPoint = clientToDiagram(event);
    if (!diagramPoint) {
      return;
    }

    if (dragState.type === "node") {
      const proposed = {
        x: diagramPoint.x - dragState.offset.x,
        y: diagramPoint.y - dragState.offset.y,
      };
      const alignment = computeNodeAlignment(
        dragState.id,
        proposed,
        alignmentEntries,
        ALIGN_THRESHOLD,
        nodeDimensions
      );
      const snappedPosition = {
        x: alignment.appliedX ? alignment.position.x : snapToGrid(alignment.position.x),
        y: alignment.appliedY ? alignment.position.y : snapToGrid(alignment.position.y),
      };
      setAlignmentGuides((prev) =>
        guidesEqual(prev, alignment.guides) ? prev : alignment.guides
      );
      setDragState({ ...dragState, current: snappedPosition, moved: true });
      setDraftNodes((prev: DraftNodes) => ({ ...prev, [dragState.id]: snappedPosition }));
    } else if (dragState.type === "edge") {
      const handleId = `edge:${dragState.id}:handle:${dragState.index}`;
      const alignment = computeNodeAlignment(
        handleId,
        diagramPoint,
        alignmentEntries,
        ALIGN_THRESHOLD,
        nodeDimensions
      );
      const snappedPoint = {
        x: alignment.appliedX ? alignment.position.x : snapToGrid(alignment.position.x),
        y: alignment.appliedY ? alignment.position.y : snapToGrid(alignment.position.y),
      };
      setAlignmentGuides((prev) =>
        guidesEqual(prev, alignment.guides) ? prev : alignment.guides
      );
      const nextPoints = dragState.points.map((point: Point, idx: number) =>
        idx === dragState.index ? snappedPoint : point
      );
      setDragState({ ...dragState, points: nextPoints, moved: true });
      setDraftEdges((prev: DraftEdges) => ({ ...prev, [dragState.id]: nextPoints }));
    } else if (dragState.type === "subgraph") {
      const targetTopLeft = {
        x: diagramPoint.x - dragState.offset.x,
        y: diagramPoint.y - dragState.offset.y,
      };
      const proposedDelta = {
        x: targetTopLeft.x - dragState.origin.x,
        y: targetTopLeft.y - dragState.origin.y,
      };
      const resolvedDelta = resolveSubgraphDelta(dragState, proposedDelta);
      const newTopLeft = {
        x: dragState.origin.x + resolvedDelta.x,
        y: dragState.origin.y + resolvedDelta.y,
      };
      const moved =
        dragState.moved ||
        Math.abs(resolvedDelta.x) > EPSILON ||
        Math.abs(resolvedDelta.y) > EPSILON;

      setAlignmentGuides((prev) => (guidesEqual(prev, EMPTY_GUIDES) ? prev : EMPTY_GUIDES));
      setDragState({
        ...dragState,
        delta: resolvedDelta,
        moved,
        offset: {
          x: diagramPoint.x - newTopLeft.x,
          y: diagramPoint.y - newTopLeft.y,
        },
      });

      setDraftNodes((prev: DraftNodes) => {
        const next: DraftNodes = { ...prev };
        for (const nodeId of dragState.members) {
          const offset = dragState.nodeOffsets[nodeId];
          if (offset) {
            next[nodeId] = {
              x: newTopLeft.x + offset.x,
              y: newTopLeft.y + offset.y,
            };
          }
        }
        return next;
      });

      setDraftSubgraphs((prev: DraftSubgraphs) => {
        const next: DraftSubgraphs = { ...prev };
        for (const subgraphId of dragState.subgraphIds) {
          next[subgraphId] = { x: resolvedDelta.x, y: resolvedDelta.y };
        }
        return next;
      });

      if (Object.keys(dragState.edgeOverrides).length > 0) {
        setDraftEdges((prev: DraftEdges) => {
          const next: DraftEdges = { ...prev };
          for (const [edgeId, basePoints] of Object.entries(dragState.edgeOverrides)) {
            next[edgeId] = basePoints.map((point) => ({
              x: point.x + resolvedDelta.x,
              y: point.y + resolvedDelta.y,
            }));
          }
          return next;
        });
      }
    }
  };

  const handlePointerUp = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (!dragState) {
      return;
    }

    const currentDrag = dragState;
    onDragStateChange?.(false);
    setAlignmentGuides((prev) => (guidesEqual(prev, EMPTY_GUIDES) ? prev : EMPTY_GUIDES));

    if (currentDrag.type === "node") {
      if (currentDrag.moved) {
        const node = diagram.nodes.find((item) => item.id === currentDrag.id);
        const current = currentDrag.current;
        const auto = node?.autoPosition;
        const result = auto && current && isClose(current, auto) ? null : current;
        onNodeMove(currentDrag.id, result);
      }
      setDraftNodes((prev: DraftNodes) => {
        const next = { ...prev };
        delete next[currentDrag.id];
        return next;
      });
    } else if (currentDrag.type === "edge") {
      if (currentDrag.moved) {
        const normalized = currentDrag.points.map((point: Point) => ({ ...point }));
        const shouldClear = normalized.length === 0;
        onEdgeMove(currentDrag.id, shouldClear ? null : normalized);
      }
      setDraftEdges((prev: DraftEdges) => {
        const next = { ...prev };
        delete next[currentDrag.id];
        return next;
      });
    } else if (currentDrag.type === "subgraph") {
      if (currentDrag.moved) {
        const nodeUpdates: Record<string, Point | null> = {};
        const edgeUpdates: Record<string, Point[]> = {};
        const finalTopLeft = {
          x: currentDrag.origin.x + currentDrag.delta.x,
          y: currentDrag.origin.y + currentDrag.delta.y,
        };
        for (const nodeId of currentDrag.members) {
          const offset = currentDrag.nodeOffsets[nodeId];
          if (!offset) {
            continue;
          }
          const node = diagram.nodes.find((item) => item.id === nodeId);
          const finalPoint = {
            x: finalTopLeft.x + offset.x,
            y: finalTopLeft.y + offset.y,
          };
          const auto = node?.autoPosition;
          nodeUpdates[nodeId] = auto && isClose(finalPoint, auto) ? null : finalPoint;
        }

        for (const [edgeId, basePoints] of Object.entries(currentDrag.edgeOverrides)) {
          if (!basePoints || basePoints.length === 0) {
            continue;
          }
          const shifted = basePoints.map((point) => ({
            x: point.x + currentDrag.delta.x,
            y: point.y + currentDrag.delta.y,
          }));
          edgeUpdates[edgeId] = shifted;
        }

        if (onLayoutUpdate) {
          const payload: LayoutUpdate = {};
          if (Object.keys(nodeUpdates).length > 0) {
            payload.nodes = nodeUpdates;
          }
          if (Object.keys(edgeUpdates).length > 0) {
            payload.edges = {};
            for (const [edgeId, points] of Object.entries(edgeUpdates)) {
              payload.edges[edgeId] = { points };
            }
          }
          if (payload.nodes || payload.edges) {
            onLayoutUpdate(payload);
          }
        } else {
          if (Object.keys(nodeUpdates).length > 0) {
            for (const [nodeId, point] of Object.entries(nodeUpdates)) {
              onNodeMove(nodeId, point);
            }
          }
          for (const [edgeId, points] of Object.entries(edgeUpdates)) {
            onEdgeMove(edgeId, points);
          }
        }
      }

      setDraftNodes((prev: DraftNodes) => {
        const next = { ...prev };
        for (const nodeId of currentDrag.members) {
          delete next[nodeId];
        }
        return next;
      });

      if (Object.keys(currentDrag.edgeOverrides).length > 0) {
        setDraftEdges((prev: DraftEdges) => {
          const next = { ...prev };
          for (const edgeId of Object.keys(currentDrag.edgeOverrides)) {
            delete next[edgeId];
          }
          return next;
        });
      }

      setDraftSubgraphs((prev: DraftSubgraphs) => {
        const next = { ...prev };
        for (const subgraphId of currentDrag.subgraphIds) {
          delete next[subgraphId];
        }
        return next;
      });
    }

    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }

    setDragState(null);
  };

  const handlePointerCancel = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (!dragState) {
      return;
    }

    const currentDrag = dragState;
    onDragStateChange?.(false);
    setAlignmentGuides((prev) => (guidesEqual(prev, EMPTY_GUIDES) ? prev : EMPTY_GUIDES));

    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
    }

    if (currentDrag.type === "node") {
      setDraftNodes((prev: DraftNodes) => {
        const next = { ...prev };
        delete next[currentDrag.id];
        return next;
      });
    } else if (currentDrag.type === "edge") {
      setDraftEdges((prev: DraftEdges) => {
        const next = { ...prev };
        delete next[currentDrag.id];
        return next;
      });
    } else if (currentDrag.type === "subgraph") {
      setDraftNodes((prev: DraftNodes) => {
        const next = { ...prev };
        for (const nodeId of currentDrag.members) {
          delete next[nodeId];
        }
        return next;
      });

      if (Object.keys(currentDrag.edgeOverrides).length > 0) {
        setDraftEdges((prev: DraftEdges) => {
          const next = { ...prev };
          for (const edgeId of Object.keys(currentDrag.edgeOverrides)) {
            delete next[edgeId];
          }
          return next;
        });
      }

      setDraftSubgraphs((prev: DraftSubgraphs) => {
        const next = { ...prev };
        for (const subgraphId of currentDrag.subgraphIds) {
          delete next[subgraphId];
        }
        return next;
      });
    }

    setDragState(null);
  };

  const handleHandleDoubleClick = (edgeId: string) => {
    onEdgeMove(edgeId, null);
  };

  const handleNodeDoubleClick = (id: string) => {
    onNodeMove(id, null);
  };

  useEffect(() => {
    if (!selectedNodeId) {
      return;
    }

    const handleKeyDown = (event: KeyboardEvent) => {
      if (!selectedNodeId) {
        return;
      }
      if (dragState) {
        return;
      }
      const { key } = event;
      if (key !== "ArrowUp" && key !== "ArrowDown" && key !== "ArrowLeft" && key !== "ArrowRight") {
        return;
      }

      const active = document.activeElement as HTMLElement | null;
      if (
        active &&
        (active.tagName === "TEXTAREA" ||
          active.tagName === "INPUT" ||
          active.isContentEditable)
      ) {
        return;
      }

      const current = finalPositions.get(selectedNodeId);
      if (!current) {
        return;
      }

      const step = event.shiftKey ? GRID_SIZE : 1;
      let deltaX = 0;
      let deltaY = 0;
      switch (key) {
        case "ArrowUp":
          deltaY = -step;
          break;
        case "ArrowDown":
          deltaY = step;
          break;
        case "ArrowLeft":
          deltaX = -step;
          break;
        case "ArrowRight":
          deltaX = step;
          break;
        default:
          break;
      }

      if (deltaX === 0 && deltaY === 0) {
        return;
      }

      event.preventDefault();

      const next = {
        x: current.x + deltaX,
        y: current.y + deltaY,
      };
      const adjusted = event.shiftKey
        ? {
          x: snapToGrid(next.x),
          y: snapToGrid(next.y),
        }
        : next;

      setDraftNodes((prev: DraftNodes) => ({ ...prev, [selectedNodeId]: adjusted }));
      setAlignmentGuides((prev) => (guidesEqual(prev, EMPTY_GUIDES) ? prev : EMPTY_GUIDES));
      onNodeMove(selectedNodeId, adjusted);
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [selectedNodeId, dragState, finalPositions, onNodeMove]);

  useEffect(() => {
    if (dragState && (dragState.type === "node" || dragState.type === "subgraph")) {
      return;
    }
    setDraftNodes((prev: DraftNodes) => {
      if (Object.keys(prev).length === 0) {
        return prev;
      }
      let mutated = false;
      const nextDraft: DraftNodes = { ...prev };
      for (const [id, point] of Object.entries(prev)) {
        const node = diagram.nodes.find((item) => item.id === id);
        if (!node) {
          delete nextDraft[id];
          mutated = true;
          continue;
        }
        const resolved = node.overridePosition ?? node.renderedPosition ?? node.autoPosition;
        if (resolved && isClose(resolved, point)) {
          delete nextDraft[id];
          mutated = true;
        }
      }
      return mutated ? nextDraft : prev;
    });
  }, [diagram.nodes, dragState]);

  useEffect(() => {
    if (dragState && dragState.type === "subgraph") {
      return;
    }
    if (Object.keys(draftSubgraphs).length === 0) {
      return;
    }
    setDraftSubgraphs((prev: DraftSubgraphs) => {
      if (Object.keys(prev).length === 0) {
        return prev;
      }
      return {};
    });
  }, [diagram.subgraphs, dragState, draftSubgraphs]);

  return (
    <div ref={wrapperRef} className="diagram-wrapper">
      <svg
        ref={svgRef}
        className="diagram"
        viewBox={`0 0 ${bounds.width} ${bounds.height}`}
        onPointerDown={handleCanvasPointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onPointerCancel={handlePointerCancel}
        onContextMenu={handleCanvasContextMenu}
      >
        {subgraphViews.map((subgraph) => {
          const offset = draftSubgraphs[subgraph.id];
          const offsetX = offset?.x ?? 0;
          const offsetY = offset?.y ?? 0;
          const effectiveTopLeft = { x: subgraph.x + offsetX, y: subgraph.y + offsetY };
          const effectiveBottomRight = {
            x: subgraph.x + subgraph.width + offsetX,
            y: subgraph.y + subgraph.height + offsetY,
          };
          const effectiveLabelPoint = {
            x: subgraph.labelX + offsetX,
            y: subgraph.labelY + offsetY,
          };
          const topLeft = toScreen(effectiveTopLeft);
          const bottomRight = toScreen({
            x: effectiveBottomRight.x,
            y: effectiveBottomRight.y,
          });
          const labelPoint = toScreen(effectiveLabelPoint);
          const width = bottomRight.x - topLeft.x;
          const height = bottomRight.y - topLeft.y;
          const dragging = dragState?.type === "subgraph" && dragState.id === subgraph.id;
          return (
            <g
              key={`subgraph-${subgraph.id}`}
              className="subgraph"
              data-id={subgraph.id}
              style={{ cursor: dragging ? "grabbing" : "grab" }}
              onPointerDown={(event: ReactPointerEvent<SVGGElement>) =>
                handleSubgraphPointerDown(subgraph.id, event)
              }
              onContextMenu={(event: ReactMouseEvent<SVGGElement>) => {
                event.preventDefault();
                closeContextMenu();
              }}
            >
              <rect
                x={topLeft.x}
                y={topLeft.y}
                width={width}
                height={height}
                rx={SUBGRAPH_BORDER_RADIUS}
                ry={SUBGRAPH_BORDER_RADIUS}
                fill={SUBGRAPH_FILL}
                fillOpacity={0.7}
                stroke={SUBGRAPH_STROKE}
                strokeWidth={1.5}
              />
              <text
                x={labelPoint.x}
                y={labelPoint.y}
                fill={SUBGRAPH_LABEL_COLOR}
                fontSize={14}
                fontWeight={600}
                textAnchor="start"
                dominantBaseline="hanging"
              >
                {subgraph.label}
              </text>
            </g>
          );
        })}

        {edges.map((view: EdgeView) => {
          const {
            edge,
            route,
            handlePoints,
            hasOverride,
            color,
            arrowDirection,
            labelHandleIndex,
            labelPoint: resolvedLabelPoint,
          } = view;
          const screenRoute = route.map(toScreen);
          const pathPoints = screenRoute.map((point: Point) => `${point.x},${point.y}`).join(" ");
          const primaryHandlePoint =
            labelHandleIndex !== null ? handlePoints[labelHandleIndex] : null;
          const labelAnchor = primaryHandlePoint ?? resolvedLabelPoint;
          const labelScreen = toScreen(labelAnchor);
          const labelHandleDragging =
            primaryHandlePoint &&
            dragState?.type === "edge" &&
            dragState.id === edge.id &&
            dragState.index === labelHandleIndex;

          const edgeSelected = selectedEdgeId === edge.id;
          const markerStart =
            arrowDirection === "backward" || arrowDirection === "both"
              ? "url(#arrow-start)"
              : undefined;
          const markerEnd =
            arrowDirection === "forward" || arrowDirection === "both"
              ? "url(#arrow-end)"
              : undefined;

          const labelDisplayPoint = labelScreen;
          const labelLines = edge.label ? normalizeLabelLines(edge.label) : [];
          const labelSize = edge.label ? measureLabelBox(labelLines) : null;
          const labelStroke = edgeSelected ? "#f472b6" : color;
          const labelBaselineStart = -((labelLines.length - 1) * EDGE_LABEL_LINE_HEIGHT) / 2;

          const renderLabelText = (pointerEvents: "none" | "auto") => {
            if (labelLines.length === 0) {
              return null;
            }
            if (labelLines.length === 1) {
              return (
                <text
                  className="edge-label"
                  textAnchor="middle"
                  fontSize={EDGE_LABEL_FONT_SIZE}
                  dominantBaseline="middle"
                  pointerEvents={pointerEvents}
                >
                  {labelLines[0]}
                </text>
              );
            }

            return (
              <text
                className="edge-label"
                textAnchor="middle"
                fontSize={EDGE_LABEL_FONT_SIZE}
                dominantBaseline="middle"
                pointerEvents={pointerEvents}
              >
                {labelLines.map((line, idx) => (
                  <tspan
                    key={`${edge.id}-label-line-${idx}`}
                    x={0}
                    y={labelBaselineStart + idx * EDGE_LABEL_LINE_HEIGHT}
                    dominantBaseline="middle"
                  >
                    {line}
                  </tspan>
                ))}
              </text>
            );
          };

          return (
            <g
              key={edge.id}
              className={edgeSelected ? "edge selected" : "edge"}
              onPointerDown={(event: ReactPointerEvent<SVGGElement>) =>
                handleEdgePointerDown(edge.id, event)
              }
              onContextMenu={(event: ReactMouseEvent<SVGGElement>) =>
                handleEdgeContextMenu(edge.id, event)
              }
            >
              {screenRoute.length === 2 ? (
                <line
                  x1={screenRoute[0].x}
                  y1={screenRoute[0].y}
                  x2={screenRoute[1].x}
                  y2={screenRoute[1].y}
                  stroke={color}
                  strokeWidth={2}
                  markerStart={markerStart}
                  markerEnd={markerEnd}
                  strokeDasharray={edge.kind === "dashed" ? "8 6" : undefined}
                  onPointerDown={(event: ReactPointerEvent<SVGLineElement>) =>
                    handleEdgePointerDown(edge.id, event)
                  }
                  onContextMenu={(event: ReactMouseEvent<SVGLineElement>) =>
                    handleEdgeContextMenu(edge.id, event)
                  }
                  onDoubleClick={(event: ReactMouseEvent<SVGLineElement>) =>
                    handleEdgeDoubleClick(edge.id, handlePoints, route, event)
                  }
                />
              ) : (
                <polyline
                  points={pathPoints}
                  fill="none"
                  stroke={color}
                  strokeWidth={2}
                  markerStart={markerStart}
                  markerEnd={markerEnd}
                  strokeDasharray={edge.kind === "dashed" ? "8 6" : undefined}
                  onPointerDown={(event: ReactPointerEvent<SVGPolylineElement>) =>
                    handleEdgePointerDown(edge.id, event)
                  }
                  onContextMenu={(event: ReactMouseEvent<SVGPolylineElement>) =>
                    handleEdgeContextMenu(edge.id, event)
                  }
                  onDoubleClick={(event: ReactMouseEvent<SVGPolylineElement>) =>
                    handleEdgeDoubleClick(edge.id, handlePoints, route, event)
                  }
                />
              )}
              {edge.label && primaryHandlePoint && labelSize ? (
                <g
                  className={`edge-label-handle${labelHandleDragging ? " edge-label-handle-active" : ""}`}
                  transform={`translate(${labelDisplayPoint.x}, ${labelDisplayPoint.y})`}
                  onPointerDown={(event: ReactPointerEvent<SVGElement>) =>
                    handleHandlePointerDown(
                      edge.id,
                      labelHandleIndex ?? 0,
                      handlePoints,
                      hasOverride,
                      event
                    )
                  }
                  onDoubleClick={(event: ReactMouseEvent<SVGGElement>) => {
                    event.stopPropagation();
                    handleHandleDoubleClick(edge.id);
                  }}
                >
                  <rect
                    x={-labelSize.width / 2}
                    y={-labelSize.height / 2}
                    width={labelSize.width}
                    height={labelSize.height}
                    rx={EDGE_LABEL_BORDER_RADIUS}
                    ry={EDGE_LABEL_BORDER_RADIUS}
                    fill={EDGE_LABEL_BACKGROUND}
                    fillOpacity={EDGE_LABEL_BACKGROUND_OPACITY}
                    stroke={labelStroke}
                    strokeWidth={1}
                    pointerEvents="none"
                  />
                  {renderLabelText("auto")}
                </g>
              ) : edge.label && labelSize ? (
                <g
                  className="edge-label-group"
                  transform={`translate(${labelDisplayPoint.x}, ${labelDisplayPoint.y})`}
                >
                  <rect
                    x={-labelSize.width / 2}
                    y={-labelSize.height / 2}
                    width={labelSize.width}
                    height={labelSize.height}
                    rx={EDGE_LABEL_BORDER_RADIUS}
                    ry={EDGE_LABEL_BORDER_RADIUS}
                    fill={EDGE_LABEL_BACKGROUND}
                    fillOpacity={EDGE_LABEL_BACKGROUND_OPACITY}
                    stroke={labelStroke}
                    strokeWidth={1}
                    pointerEvents="none"
                  />
                  {renderLabelText("none")}
                </g>
              ) : null}
              {handlePoints
                .map((point: Point, index: number) => ({ point, index }))
                .filter(({ index }) => labelHandleIndex === null || index !== labelHandleIndex)
                .map(({ point, index }) => {
                  const screen = toScreen(point);
                  return (
                    <circle
                      key={`${edge.id}-handle-${index}`}
                      className={hasOverride ? "handle active" : "handle"}
                      cx={screen.x}
                      cy={screen.y}
                      r={HANDLE_RADIUS}
                      onPointerDown={(event: ReactPointerEvent<SVGCircleElement>) =>
                        handleHandlePointerDown(edge.id, index, handlePoints, hasOverride, event)
                      }
                      onDoubleClick={(event: ReactMouseEvent<SVGCircleElement>) => {
                        event.stopPropagation();
                        handleHandleDoubleClick(edge.id);
                      }}
                    />
                  );
                })}
            </g>
          );
        })}

        {nodeEntries.map(([id, position]) => {
          const screen = toScreen(position);
          const node = diagram.nodes.find((item) => item.id === id);
          if (!node) {
            return null;
          }

          const defaultFill = SHAPE_COLORS[node.shape] ?? "#ffffff";
          const baseFillColor = node.fillColor ?? defaultFill;
          const hasImage = Boolean(node.image);
          const imageFillColor = hasImage
            ? node.imageFillColor ?? "#ffffff"
            : baseFillColor;
          const labelFillColor = hasImage
            ? node.labelFillColor ?? baseFillColor
            : baseFillColor;
          const fillColor = imageFillColor;
          const strokeColor = node.strokeColor ?? DEFAULT_NODE_STROKE;
          const textColor = node.textColor ?? DEFAULT_NODE_TEXT;
          const nodeStyle = {
            "--node-fill": fillColor,
            "--node-stroke": strokeColor,
            "--node-text": textColor,
          } as CSSProperties;
          const nodeSelected = selectedNodeId === id;
          const nodeWidth = node.width ?? DEFAULT_NODE_WIDTH;
          const nodeHeight = node.height ?? DEFAULT_NODE_HEIGHT;
          const halfWidth = nodeWidth / 2;
          const halfHeight = nodeHeight / 2;

          const imageData = node.image ?? null;
          const imagePadding = imageData
            ? Math.max(0, Number.isFinite(imageData.padding) ? imageData.padding : 0)
            : 0;
          const clipId = svgSafeId("node-clip-", id);
          const labelLines = normalizeLabelLines(node.label);
          const hasLabel = labelLines.length > 0;
          const labelLineCount = Math.max(1, labelLines.length);
          const labelAreaHeight = imageData
            ? Math.max(NODE_LABEL_HEIGHT, labelLineCount * NODE_TEXT_LINE_HEIGHT)
            : 0;
          const imageHeight = Math.max(0, nodeHeight - labelAreaHeight - imagePadding * 2);
          const imageWidth = Math.max(0, nodeWidth - imagePadding * 2);

          const shapeComponents = (() => {
            switch (node.shape) {
              case "rectangle": {
                const shape = (
                  <rect
                    x={-halfWidth}
                    y={-halfHeight}
                    width={nodeWidth}
                    height={nodeHeight}
                    rx={8}
                    ry={8}
                    fill={fillColor}
                  />
                );
                const clip = (
                  <rect x={-halfWidth} y={-halfHeight} width={nodeWidth} height={nodeHeight} rx={8} ry={8} />
                );
                const outline = (
                  <rect
                    x={-halfWidth}
                    y={-halfHeight}
                    width={nodeWidth}
                    height={nodeHeight}
                    rx={8}
                    ry={8}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
              case "stadium": {
                const shape = (
                  <rect
                    x={-halfWidth}
                    y={-halfHeight}
                    width={nodeWidth}
                    height={nodeHeight}
                    rx={30}
                    ry={30}
                    fill={fillColor}
                  />
                );
                const clip = (
                  <rect x={-halfWidth} y={-halfHeight} width={nodeWidth} height={nodeHeight} rx={30} ry={30} />
                );
                const outline = (
                  <rect
                    x={-halfWidth}
                    y={-halfHeight}
                    width={nodeWidth}
                    height={nodeHeight}
                    rx={30}
                    ry={30}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
              case "circle": {
                const shape = (
                  <ellipse
                    cx={0}
                    cy={0}
                    rx={halfWidth}
                    ry={halfHeight}
                    fill={fillColor}
                  />
                );
                const clip = <ellipse cx={0} cy={0} rx={halfWidth} ry={halfHeight} />;
                const outline = (
                  <ellipse
                    cx={0}
                    cy={0}
                    rx={halfWidth}
                    ry={halfHeight}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
              case "double-circle": {
                const innerRx = Math.max(halfWidth - 6, halfWidth * 0.65);
                const innerRy = Math.max(halfHeight - 6, halfHeight * 0.65);
                const shape = (
                  <>
                    <ellipse
                      cx={0}
                      cy={0}
                      rx={halfWidth}
                      ry={halfHeight}
                      fill={fillColor}
                    />
                  </>
                );
                const clip = <ellipse cx={0} cy={0} rx={halfWidth} ry={halfHeight} />;
                const outline = (
                  <>
                    <ellipse
                      cx={0}
                      cy={0}
                      rx={halfWidth}
                      ry={halfHeight}
                      fill="none"
                      stroke={strokeColor}
                      strokeWidth={2}
                    />
                    <ellipse
                      cx={0}
                      cy={0}
                      rx={innerRx}
                      ry={innerRy}
                      fill="none"
                      stroke={strokeColor}
                      strokeWidth={2}
                    />
                  </>
                );
                return { shape, clip, outline };
              }
              case "diamond": {
                const points = polygonPoints([
                  [0, -halfHeight],
                  [halfWidth, 0],
                  [0, halfHeight],
                  [-halfWidth, 0],
                ]);
                const shape = <polygon points={points} fill={fillColor} />;
                const clip = <polygon points={points} />;
                const outline = (
                  <polygon
                    points={points}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
              case "subroutine": {
                const inset = 12;
                const shape = (
                  <rect
                    x={-halfWidth}
                    y={-halfHeight}
                    width={nodeWidth}
                    height={nodeHeight}
                    rx={8}
                    ry={8}
                    fill={fillColor}
                  />
                );
                const clip = <rect x={-halfWidth} y={-halfHeight} width={nodeWidth} height={nodeHeight} rx={8} ry={8} />;
                const outline = (
                  <>
                    <rect
                      x={-halfWidth}
                      y={-halfHeight}
                      width={nodeWidth}
                      height={nodeHeight}
                      rx={8}
                      ry={8}
                      fill="none"
                      stroke={strokeColor}
                      strokeWidth={2}
                    />
                    <line
                      x1={-halfWidth + inset}
                      y1={-halfHeight}
                      x2={-halfWidth + inset}
                      y2={halfHeight}
                      stroke={strokeColor}
                      strokeWidth={2}
                    />
                    <line
                      x1={halfWidth - inset}
                      y1={-halfHeight}
                      x2={halfWidth - inset}
                      y2={halfHeight}
                      stroke={strokeColor}
                      strokeWidth={2}
                    />
                  </>
                );
                return { shape, clip, outline };
              }
              case "cylinder": {
                const rx = halfWidth;
                const ry = nodeHeight / 6;
                const top = -halfHeight;
                const bottom = halfHeight;
                const topCenter = top + ry;
                const bottomCenter = bottom - ry;
                const bodyPath = `M ${-halfWidth},${topCenter} A ${rx},${ry} 0 0 1 ${halfWidth},${topCenter} L ${halfWidth},${bottomCenter} A ${rx},${ry} 0 0 1 ${-halfWidth},${bottomCenter} Z`;
                const topPath = `M ${-halfWidth},${topCenter} A ${rx},${ry} 0 0 1 ${halfWidth},${topCenter}`;
                const shape = <path d={bodyPath} fill={fillColor} />;
                const clip = <path d={bodyPath} />;
                const outline = (
                  <>
                    <path d={bodyPath} fill="none" stroke={strokeColor} strokeWidth={2} />
                    <path d={topPath} fill="none" stroke={strokeColor} strokeWidth={2} />
                  </>
                );
                return { shape, clip, outline };
              }
              case "hexagon": {
                const offset = nodeWidth * 0.25;
                const points = polygonPoints([
                  [-halfWidth + offset, -halfHeight],
                  [halfWidth - offset, -halfHeight],
                  [halfWidth, 0],
                  [halfWidth - offset, halfHeight],
                  [-halfWidth + offset, halfHeight],
                  [-halfWidth, 0],
                ]);
                const shape = <polygon points={points} fill={fillColor} />;
                const clip = <polygon points={points} />;
                const outline = (
                  <polygon
                    points={points}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
              case "parallelogram": {
                const skew = nodeHeight * 0.35;
                const points = polygonPoints([
                  [-halfWidth + skew, -halfHeight],
                  [halfWidth, -halfHeight],
                  [halfWidth - skew, halfHeight],
                  [-halfWidth, halfHeight],
                ]);
                const shape = <polygon points={points} fill={fillColor} />;
                const clip = <polygon points={points} />;
                const outline = (
                  <polygon
                    points={points}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
              case "parallelogram-alt": {
                const skew = nodeHeight * 0.35;
                const points = polygonPoints([
                  [-halfWidth, -halfHeight],
                  [halfWidth - skew, -halfHeight],
                  [halfWidth, halfHeight],
                  [-halfWidth + skew, halfHeight],
                ]);
                const shape = <polygon points={points} fill={fillColor} />;
                const clip = <polygon points={points} />;
                const outline = (
                  <polygon
                    points={points}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
              case "trapezoid": {
                const topInset = nodeWidth * 0.22;
                const bottomInset = nodeWidth * 0.08;
                const points = polygonPoints([
                  [-halfWidth + topInset, -halfHeight],
                  [halfWidth - topInset, -halfHeight],
                  [halfWidth - bottomInset, halfHeight],
                  [-halfWidth + bottomInset, halfHeight],
                ]);
                const shape = <polygon points={points} fill={fillColor} />;
                const clip = <polygon points={points} />;
                const outline = (
                  <polygon
                    points={points}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
              case "trapezoid-alt": {
                const topInset = nodeWidth * 0.08;
                const bottomInset = nodeWidth * 0.22;
                const points = polygonPoints([
                  [-halfWidth + topInset, -halfHeight],
                  [halfWidth - topInset, -halfHeight],
                  [halfWidth - bottomInset, halfHeight],
                  [-halfWidth + bottomInset, halfHeight],
                ]);
                const shape = <polygon points={points} fill={fillColor} />;
                const clip = <polygon points={points} />;
                const outline = (
                  <polygon
                    points={points}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
              case "asymmetric": {
                const skew = nodeHeight * 0.45;
                const points = polygonPoints([
                  [-halfWidth, -halfHeight],
                  [halfWidth - skew, -halfHeight],
                  [halfWidth, 0],
                  [halfWidth - skew, halfHeight],
                  [-halfWidth, halfHeight],
                ]);
                const shape = <polygon points={points} fill={fillColor} />;
                const clip = <polygon points={points} />;
                const outline = (
                  <polygon
                    points={points}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
              default: {
                const shape = (
                  <rect
                    x={-halfWidth}
                    y={-halfHeight}
                    width={nodeWidth}
                    height={nodeHeight}
                    rx={8}
                    ry={8}
                    fill={fillColor}
                  />
                );
                const clip = (
                  <rect x={-halfWidth} y={-halfHeight} width={nodeWidth} height={nodeHeight} rx={8} ry={8} />
                );
                const outline = (
                  <rect
                    x={-halfWidth}
                    y={-halfHeight}
                    width={nodeWidth}
                    height={nodeHeight}
                    rx={8}
                    ry={8}
                    fill="none"
                    stroke={strokeColor}
                    strokeWidth={2}
                  />
                );
                return { shape, clip, outline };
              }
            }
          })();

          const shapeElement = shapeComponents.shape;
          const clipShapeElement = shapeComponents.clip ?? (
            <rect x={-halfWidth} y={-halfHeight} width={nodeWidth} height={nodeHeight} rx={8} ry={8} />
          );
          const outlineElement = shapeComponents.outline ?? null;

          const renderLabel = () => {
            if (!hasLabel) {
              return null;
            }

            if (imageData) {
              if (labelLines.length === 1) {
                const baseline = -halfHeight + labelAreaHeight / 2;
                return (
                  <text
                    x={0}
                    y={baseline}
                    fill={textColor}
                    fontSize={14}
                    textAnchor="middle"
                    dominantBaseline="middle"
                  >
                    {labelLines[0]}
                  </text>
                );
              }

              const totalTextHeight = NODE_TEXT_LINE_HEIGHT * labelLines.length;
              const labelTop = -halfHeight;
              const startY =
                labelTop + (labelAreaHeight - totalTextHeight) / 2 + NODE_TEXT_LINE_HEIGHT / 2;
              return (
                <text x={0} fill={textColor} fontSize={14} textAnchor="middle">
                  {labelLines.map((line, idx) => {
                    const lineY = startY + NODE_TEXT_LINE_HEIGHT * idx;
                    return (
                      <tspan key={idx} x={0} y={lineY} dominantBaseline="middle">
                        {line}
                      </tspan>
                    );
                  })}
                </text>
              );
            }

            if (labelLines.length === 1) {
              return (
                <text
                  x={0}
                  y={0}
                  fill={textColor}
                  fontSize={14}
                  textAnchor="middle"
                  dominantBaseline="middle"
                >
                  {labelLines[0]}
                </text>
              );
            }

            const startY = -NODE_TEXT_LINE_HEIGHT * (labelLines.length - 1) / 2;
            return (
              <text x={0} fill={textColor} fontSize={14} textAnchor="middle">
                {labelLines.map((line, idx) => {
                  const lineY = startY + NODE_TEXT_LINE_HEIGHT * idx;
                  return (
                    <tspan key={idx} x={0} y={lineY} dominantBaseline="middle">
                      {line}
                    </tspan>
                  );
                })}
              </text>
            );
          };

          return (
            <g
              key={id}
              className={nodeSelected ? "node selected" : "node"}
              transform={`translate(${screen.x}, ${screen.y})`}
              style={nodeStyle}
              onPointerDown={(event: ReactPointerEvent<SVGGElement>) =>
                handleNodePointerDown(id, event)
              }
              onContextMenu={(event: ReactMouseEvent<SVGGElement>) =>
                handleNodeContextMenu(id, event)
              }
              onDoubleClick={() => handleNodeDoubleClick(id)}
            >
              {imageData ? (
                <defs>
                  <clipPath id={clipId}>{clipShapeElement}</clipPath>
                </defs>
              ) : null}
              {shapeElement}
              {imageData && labelAreaHeight > 0 ? (
                <rect
                  x={-halfWidth}
                  y={-halfHeight}
                  width={nodeWidth}
                  height={labelAreaHeight}
                  fill={labelFillColor}
                  clipPath={`url(#${clipId})`}
                />
              ) : null}
              {imageData && imageHeight > 0.5 && imageWidth > 0.5 ? (
                <image
                  x={-halfWidth + imagePadding}
                  y={-halfHeight + labelAreaHeight + imagePadding}
                  width={imageWidth}
                  height={imageHeight}
                  href={`data:${imageData.mimeType};base64,${imageData.data}`}
                  clipPath={`url(#${clipId})`}
                  preserveAspectRatio="xMidYMid slice"
                />
              ) : null}
              {outlineElement}
              {renderLabel()}
            </g>
          );
        })}

        {verticalGuide
          ? (() => {
            const start = toScreen({ x: verticalGuide.x, y: verticalGuide.y1 });
            const end = toScreen({ x: verticalGuide.x, y: verticalGuide.y2 });
            return (
              <line
                key="vertical-guide"
                className={`alignment-guide alignment-guide-vertical alignment-guide-${verticalGuide.kind}`}
                x1={start.x}
                y1={start.y}
                x2={end.x}
                y2={end.y}
              />
            );
          })()
          : null}
        {horizontalGuide
          ? (() => {
            const start = toScreen({ x: horizontalGuide.x1, y: horizontalGuide.y });
            const end = toScreen({ x: horizontalGuide.x2, y: horizontalGuide.y });
            return (
              <line
                key="horizontal-guide"
                className={`alignment-guide alignment-guide-horizontal alignment-guide-${horizontalGuide.kind}`}
                x1={start.x}
                y1={start.y}
                x2={end.x}
                y2={end.y}
              />
            );
          })()
          : null}

        <defs>
          <marker
            id="arrow-end"
            markerWidth="12"
            markerHeight="12"
            refX="10"
            refY="6"
            orient="auto"
            markerUnits="strokeWidth"
          >
            <path d="M2,2 L10,6 L2,10 z" fill="context-stroke" />
          </marker>
          <marker
            id="arrow-start"
            markerWidth="12"
            markerHeight="12"
            refX="2"
            refY="6"
            orient="auto"
            markerUnits="strokeWidth"
          >
            <path d="M10,2 L2,6 L10,10 z" fill="context-stroke" />
          </marker>
        </defs>
      </svg>
      {contextMenu.visible && contextMenu.target ? (
        <div
          className="context-menu"
          style={{ top: contextMenu.y, left: contextMenu.x }}
          role="menu"
        >
          <button
            type="button"
            onClick={(event) => {
              event.stopPropagation();
              handleContextMenuDelete();
            }}
          >
            Delete {contextMenu.target.type}
          </button>
        </div>
      ) : null}
    </div>
  );
}
