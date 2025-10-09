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
  Point,
} from "../lib/types";

const NODE_WIDTH = 140;
const NODE_HEIGHT = 60;
const LAYOUT_MARGIN = 80;
const HANDLE_RADIUS = 6;
const EPSILON = 0.5;

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

interface EdgeView {
  edge: EdgeData;
  route: Point[];
  handlePoints: Point[];
  hasOverride: boolean;
  color: string;
  arrowDirection: EdgeArrowDirection;
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

  const bounds = useMemo(() => {
    let minX = Infinity;
    let minY = Infinity;
    let maxX = -Infinity;
    let maxY = -Infinity;

    for (const position of finalPositions.values()) {
      minX = Math.min(minX, position.x - NODE_WIDTH / 2);
      maxX = Math.max(maxX, position.x + NODE_WIDTH / 2);
      minY = Math.min(minY, position.y - NODE_HEIGHT / 2);
      maxY = Math.max(maxY, position.y + NODE_HEIGHT / 2);
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
  }, [finalPositions]);

  const edges = useMemo<EdgeView[]>(() => {
    return diagram.edges
      .map((edge) => {
        const from = finalPositions.get(edge.from);
        const to = finalPositions.get(edge.to);
        if (!from || !to) {
          return null;
        }

        const overridePoints = draftEdges[edge.id] ?? edge.overridePoints ?? [];
        const hasOverride = overridePoints.length > 0;
        const route = [from, ...overridePoints, to];
        const handlePoints = hasOverride ? overridePoints : [midpoint(from, to)];
        const color = edge.color ?? DEFAULT_EDGE_COLOR;
        const arrowDirection = edge.arrowDirection ?? "forward";

        return {
          edge,
          route,
          handlePoints,
          hasOverride,
          color,
          arrowDirection,
        };
      })
      .filter((value): value is EdgeView => value !== null);
  }, [diagram.edges, draftEdges, finalPositions]);

  const nodeEntries = useMemo<[string, Point][]>(() => {
    return Array.from(finalPositions.entries());
  }, [finalPositions]);

  const toScreen = (point: Point) => ({
    x: point.x + bounds.offsetX,
    y: point.y + bounds.offsetY,
  });

  const clientToDiagram = (event: ReactPointerEvent): Point | null => {
    const svg = svgRef.current;
    if (!svg) {
      return null;
    }
    const point = svg.createSVGPoint();
    point.x = event.clientX;
    point.y = event.clientY;
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
    event: ReactPointerEvent<SVGCircleElement>
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

  const handlePointerMove = (event: ReactPointerEvent<SVGSVGElement>) => {
    if (!dragState) {
      return;
    }
    const diagramPoint = clientToDiagram(event);
    if (!diagramPoint) {
      return;
    }

    if (dragState.type === "node") {
      const next = {
        x: diagramPoint.x - dragState.offset.x,
        y: diagramPoint.y - dragState.offset.y,
      };
      setDragState({ ...dragState, current: next, moved: true });
      setDraftNodes((prev: DraftNodes) => ({ ...prev, [dragState.id]: next }));
    } else if (dragState.type === "edge") {
      const nextPoints = dragState.points.map((point: Point, idx: number) =>
        idx === dragState.index ? diagramPoint : point
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
          const { edge, route, handlePoints, hasOverride, color, arrowDirection } = view;
          const screenRoute = route.map(toScreen);
          const pathPoints = screenRoute.map((point: Point) => `${point.x},${point.y}`).join(" ");
          const from = finalPositions.get(edge.from);
          const to = finalPositions.get(edge.to);
          if (!from || !to) {
            return null;
          }

          const labelPoint = centroid(route);
          const labelScreen = toScreen(labelPoint);

          const edgeSelected = selectedEdgeId === edge.id;
          const markerStart =
            arrowDirection === "backward" || arrowDirection === "both"
              ? "url(#arrow-start)"
              : undefined;
          const markerEnd =
            arrowDirection === "forward" || arrowDirection === "both"
              ? "url(#arrow-end)"
              : undefined;

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
                />
              )}
              {edge.label && (
                <text className="edge-label" x={labelScreen.x} y={labelScreen.y - 10} textAnchor="middle">
                  {edge.label}
                </text>
              )}
              {handlePoints.map((point: Point, index: number) => {
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
                    onDoubleClick={() => handleHandleDoubleClick(edge.id)}
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
