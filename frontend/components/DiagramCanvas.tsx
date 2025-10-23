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
  Size,
  Point,
} from "../lib/types";

const NODE_WIDTH = 140;
const NODE_HEIGHT = 60;
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

const SHAPE_COLORS: Record<DiagramData["nodes"][number]["shape"], string> = {
  rectangle: "#FDE68A", // pastel amber
  stadium: "#C4F1F9", // pastel cyan
  circle: "#E9D8FD", // pastel purple
  diamond: "#FBCFE8", // pastel pink
};

const DEFAULT_NODE_STROKE = "#2d3748";
const DEFAULT_NODE_TEXT = "#1a202c";
const DEFAULT_EDGE_COLOR = "#2d3748";

interface DiagramCanvasProps {
  diagram: DiagramData;
  onNodeMove: (id: string, position: Point | null) => void;
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

type DragState = NodeDragState | EdgeDragState | null;

type DraftNodes = Record<string, Point>;
type DraftEdges = Record<string, Point[]>;

interface NodeBox {
  left: number;
  right: number;
  centerX: number;
  top: number;
  bottom: number;
  centerY: number;
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

function createNodeBox(position: Point): NodeBox {
  return {
    left: position.x - NODE_WIDTH / 2,
    right: position.x + NODE_WIDTH / 2,
    centerX: position.x,
    top: position.y - NODE_HEIGHT / 2,
    bottom: position.y + NODE_HEIGHT / 2,
    centerY: position.y,
  };
}

function computeNodeAlignment(
  nodeId: string,
  proposed: Point,
  nodes: readonly [string, Point][],
  threshold: number
): AlignmentResult {
  const movingBox = createNodeBox(proposed);
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
    const otherBox = createNodeBox(point);

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
      const alignedBox = createNodeBox({ x: alignedX, y: proposed.y });
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
      const alignedBox = createNodeBox({ x: proposed.x, y: alignedY });
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
  const finalBox = createNodeBox(finalPosition);

  const verticalGuide = guides.vertical;
  if (verticalGuide) {
    const targetPoint = nodes.find((entry) => entry[0] === verticalGuide.targetId)?.[1];
    if (targetPoint) {
      const targetBox = createNodeBox(targetPoint);
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
      const targetBox = createNodeBox(targetPoint);
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

    for (const position of finalPositions.values()) {
      extend(position, NODE_WIDTH / 2, NODE_HEIGHT / 2);
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

    if (!Number.isFinite(minX)) {
      minX = -NODE_WIDTH / 2;
      maxX = NODE_WIDTH / 2;
      minY = -NODE_HEIGHT / 2;
      maxY = NODE_HEIGHT / 2;
    }

    const width = Math.max(maxX - minX, NODE_WIDTH) + LAYOUT_MARGIN * 2;
    const height = Math.max(maxY - minY, NODE_HEIGHT) + LAYOUT_MARGIN * 2;
    const offsetX = LAYOUT_MARGIN - minX;
    const offsetY = LAYOUT_MARGIN - minY;

    return { width, height, offsetX, offsetY };
  }, [edges, finalPositions]);

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
      const alignment = computeNodeAlignment(dragState.id, proposed, alignmentEntries, ALIGN_THRESHOLD);
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
      const alignment = computeNodeAlignment(handleId, diagramPoint, alignmentEntries, ALIGN_THRESHOLD);
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
    if (dragState && dragState.type === "node") {
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
          const fillColor = node.fillColor ?? defaultFill;
          const strokeColor = node.strokeColor ?? DEFAULT_NODE_STROKE;
          const textColor = node.textColor ?? DEFAULT_NODE_TEXT;
          const nodeStyle = {
            "--node-fill": fillColor,
            "--node-stroke": strokeColor,
            "--node-text": textColor,
          } as CSSProperties;
          const nodeSelected = selectedNodeId === id;

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
              {node.shape === "rectangle" && (
                <rect
                  x={-NODE_WIDTH / 2}
                  y={-NODE_HEIGHT / 2}
                  width={NODE_WIDTH}
                  height={NODE_HEIGHT}
                  rx={8}
                  ry={8}
                />
              )}
              {node.shape === "stadium" && (
                <rect
                  x={-NODE_WIDTH / 2}
                  y={-NODE_HEIGHT / 2}
                  width={NODE_WIDTH}
                  height={NODE_HEIGHT}
                  rx={30}
                  ry={30}
                />
              )}
              {node.shape === "circle" && (
                <ellipse cx={0} cy={0} rx={NODE_WIDTH / 2} ry={NODE_HEIGHT / 2} />
              )}
              {node.shape === "diamond" && (
                <polygon
                  points={`0,${-NODE_HEIGHT / 2} ${NODE_WIDTH / 2},0 0,${NODE_HEIGHT / 2} ${-NODE_WIDTH / 2},0`}
                />
              )}
              <text textAnchor="middle" dominantBaseline="middle">
                {node.label}
              </text>
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
