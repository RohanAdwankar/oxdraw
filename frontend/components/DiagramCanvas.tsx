'use client';

import { useMemo, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import { DiagramData, EdgeData, Point } from "../lib/types";

const NODE_WIDTH = 140;
const NODE_HEIGHT = 60;
const LAYOUT_MARGIN = 80;
const HANDLE_RADIUS = 6;
const EPSILON = 0.5;

interface DiagramCanvasProps {
  diagram: DiagramData;
  onNodeMove: (id: string, position: Point | null) => void;
  onEdgeMove: (id: string, points: Point[] | null) => void;
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
}

function midpoint(a: Point, b: Point): Point {
  return { x: (a.x + b.x) / 2, y: (a.y + b.y) / 2 };
}

function isClose(a: Point, b: Point): boolean {
  return Math.abs(a.x - b.x) < EPSILON && Math.abs(a.y - b.y) < EPSILON;
}

export default function DiagramCanvas({ diagram, onNodeMove, onEdgeMove }: DiagramCanvasProps) {
  const svgRef = useRef<SVGSVGElement | null>(null);
  const [dragState, setDragState] = useState<DragState>(null);
  const [draftNodes, setDraftNodes] = useState<DraftNodes>({});
  const [draftEdges, setDraftEdges] = useState<DraftEdges>({});

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

        return {
          edge,
          route,
          handlePoints,
          hasOverride,
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

  const handleNodePointerDown = (id: string, event: ReactPointerEvent<SVGGElement>) => {
    event.preventDefault();
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
    setDragState({ type: "node", id, offset, current, moved: false });
    setDraftNodes((prev: DraftNodes) => ({ ...prev, [id]: current }));
    event.currentTarget.setPointerCapture(event.pointerId);
  };

  const handleHandlePointerDown = (
    edgeId: string,
    index: number,
    availablePoints: Point[],
    hasOverride: boolean,
    event: ReactPointerEvent<SVGCircleElement>
  ) => {
    event.preventDefault();
    const basePoints = hasOverride
      ? availablePoints.map((point: Point) => ({ ...point }))
      : [availablePoints[index] ?? availablePoints[0]];
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

    if (dragState.type === "node") {
      const node = diagram.nodes.find((item) => item.id === dragState.id);
      const current = dragState.current;
      const auto = node?.autoPosition;
      const result = auto && current && isClose(current, auto) ? null : current;
      onNodeMove(dragState.id, dragState.moved ? result : null);
      setDraftNodes((prev: DraftNodes) => {
        const next = { ...prev };
        delete next[dragState.id];
        return next;
      });
    } else if (dragState.type === "edge") {
      const finalPoints = dragState.points;
      if (!dragState.moved && !dragState.hasOverride) {
        setDraftEdges((prev: DraftEdges) => {
          const next = { ...prev };
          delete next[dragState.id];
          return next;
        });
      } else {
        const normalized = finalPoints.map((point: Point) => ({ ...point }));
        const shouldClear = normalized.length === 0;
        onEdgeMove(dragState.id, shouldClear ? null : normalized);
        setDraftEdges((prev: DraftEdges) => {
          const next = { ...prev };
          delete next[dragState.id];
          return next;
        });
      }
    }

    if (event.currentTarget.hasPointerCapture(event.pointerId)) {
      event.currentTarget.releasePointerCapture(event.pointerId);
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
    <svg
      ref={svgRef}
      className="diagram"
      viewBox={`0 0 ${bounds.width} ${bounds.height}`}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
    >
      {edges.map((view: EdgeView) => {
        const { edge, route, handlePoints, hasOverride } = view;
        const screenRoute = route.map(toScreen);
        const pathPoints = screenRoute.map((point: Point) => `${point.x},${point.y}`).join(" ");
        const from = finalPositions.get(edge.from);
        const to = finalPositions.get(edge.to);
        if (!from || !to) {
          return null;
        }

        return (
          <g key={edge.id} className="edge">
            {screenRoute.length === 2 ? (
              <line
                x1={screenRoute[0].x}
                y1={screenRoute[0].y}
                x2={screenRoute[1].x}
                y2={screenRoute[1].y}
                stroke="#2d3748"
                strokeWidth={2}
                markerEnd="url(#arrow)"
                strokeDasharray={edge.kind === "dashed" ? "8 6" : undefined}
              />
            ) : (
              <polyline
                points={pathPoints}
                fill="none"
                stroke="#2d3748"
                strokeWidth={2}
                markerEnd="url(#arrow)"
                strokeDasharray={edge.kind === "dashed" ? "8 6" : undefined}
              />
            )}
            {edge.label && (
              <text
                className="edge-label"
                x={screenRoute[Math.floor(screenRoute.length / 2)].x}
                y={screenRoute[Math.floor(screenRoute.length / 2)].y - 10}
                textAnchor="middle"
              >
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
        return (
          <g
            key={id}
            className="node"
            transform={`translate(${screen.x}, ${screen.y})`}
            onPointerDown={(event: ReactPointerEvent<SVGGElement>) => handleNodePointerDown(id, event)}
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
        <marker id="arrow" markerWidth="12" markerHeight="12" refX="10" refY="6" orient="auto">
          <path d="M2,2 L10,6 L2,10 z" fill="#2d3748" />
        </marker>
      </defs>
    </svg>
  );
}
