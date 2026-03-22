'use client';

import { useCallback, useEffect, useRef, useState } from "react";
import type { PointerEvent as ReactPointerEvent } from "react";
import type { WheelEvent as ReactWheelEvent } from "react";
import type { DiagramCanvasProps } from "./diagramCanvasTypes";
import { createWasmEditor, type WasmEditorCore } from "../lib/wasmEditor";
import type { LayoutUpdate, Point } from "../lib/types";

type DragKind = "node" | "edge" | "subgraph" | "gantt";

interface ActiveDrag {
  kind: DragKind;
  id: string;
  pointerId: number;
}

interface EdgeVm {
  id: string;
  overridePoints?: Point[];
}

interface DiagramVm {
  edges?: EdgeVm[];
}

function toDiagramPoint(svg: SVGSVGElement, clientX: number, clientY: number): Point | null {
  const ctm = svg.getScreenCTM();
  if (!ctm) {
    return null;
  }
  const point = svg.createSVGPoint();
  point.x = clientX;
  point.y = clientY;
  const transformed = point.matrixTransform(ctm.inverse());
  return { x: transformed.x, y: transformed.y };
}

function applyLayoutPatch(
  patch: unknown,
  onLayoutUpdate: DiagramCanvasProps["onLayoutUpdate"],
  onNodeMove: DiagramCanvasProps["onNodeMove"],
  onEdgeMove: DiagramCanvasProps["onEdgeMove"]
) {
  if (!patch || typeof patch !== "object") {
    return;
  }

  const payload = patch as {
    nodes?: Record<string, Point | null>;
    edges?: Record<string, { points?: Point[] | null } | null>;
    ganttTasks?: Record<string, { startDay?: number; endDay?: number } | null>;
  };

  const hasNodes = payload.nodes && Object.keys(payload.nodes).length > 0;
  const hasEdges = payload.edges && Object.keys(payload.edges).length > 0;
  const hasGanttTasks = payload.ganttTasks && Object.keys(payload.ganttTasks).length > 0;

  if (!hasNodes && !hasEdges && !hasGanttTasks) {
    return;
  }

  if (onLayoutUpdate) {
    const update: LayoutUpdate = {};
    if (payload.nodes) {
      update.nodes = payload.nodes;
    }
    if (payload.edges) {
      const normalized: Record<string, { points?: Point[] | null }> = {};
      for (const [edgeId, value] of Object.entries(payload.edges)) {
        normalized[edgeId] = value ?? { points: null };
      }
      update.edges = normalized;
    }
    if (payload.ganttTasks) {
      update.ganttTasks = payload.ganttTasks;
    }
    onLayoutUpdate(update);
    return;
  }

  if (payload.nodes) {
    for (const [nodeId, value] of Object.entries(payload.nodes)) {
      onNodeMove(nodeId, value);
    }
  }
  if (payload.edges) {
    for (const [edgeId, value] of Object.entries(payload.edges)) {
      onEdgeMove(edgeId, value?.points ?? null);
    }
  }
}

function pickEdgeHandleIndex(core: WasmEditorCore, edgeId: string, point: Point): number {
  const vm = core.viewModel() as DiagramVm;
  const edge = vm.edges?.find((entry) => entry.id === edgeId);
  const points = edge?.overridePoints ?? [];
  if (points.length === 0) {
    return 0;
  }
  let bestIndex = 0;
  let bestDistance = Number.POSITIVE_INFINITY;
  for (let index = 0; index < points.length; index += 1) {
    const candidate = points[index];
    const dx = candidate.x - point.x;
    const dy = candidate.y - point.y;
    const distance = dx * dx + dy * dy;
    if (distance < bestDistance) {
      bestDistance = distance;
      bestIndex = index;
    }
  }
  return bestIndex;
}

const NUDGE_PIXELS = 10;
const ZOOM_MIN = 0.25;
const ZOOM_MAX = 3;
const ZOOM_STEP = 1.15;

export default function WasmDiagramCanvas({
  diagram,
  onNodeMove,
  onEdgeMove,
  onLayoutUpdate,
  selectedNodeId,
  selectedEdgeId,
  onSelectNode,
  onSelectEdge,
  onDragStateChange,
}: DiagramCanvasProps) {
  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const [svgMarkup, setSvgMarkup] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [transform, setTransform] = useState({ x: 0, y: 0, scale: 1 });
  const coreRef = useRef<WasmEditorCore | null>(null);
  const dragRef = useRef<ActiveDrag | null>(null);

  const renderFromCore = useCallback(() => {
    if (!coreRef.current) {
      return;
    }
    setSvgMarkup(coreRef.current.renderSvg());
  }, []);

  const zoomIn = useCallback(() => {
    setTransform((prev) => ({ ...prev, scale: Math.min(ZOOM_MAX, prev.scale * ZOOM_STEP) }));
  }, []);

  const zoomOut = useCallback(() => {
    setTransform((prev) => ({ ...prev, scale: Math.max(ZOOM_MIN, prev.scale / ZOOM_STEP) }));
  }, []);

  const resetZoom = useCallback(() => {
    setTransform({ x: 0, y: 0, scale: 1 });
  }, []);

  useEffect(() => {
    let cancelled = false;
    const init = async () => {
      try {
        const core = await createWasmEditor(diagram.source, diagram.background);
        if (cancelled) {
          return;
        }
        coreRef.current = core;
        dragRef.current = null;
        setError(null);
        setSvgMarkup(core.renderSvg());
      } catch (err) {
        if (cancelled) {
          return;
        }
        const message = err instanceof Error ? err.message : "Failed to initialize WASM editor";
        setError(message);
      }
    };
    void init();
    return () => {
      cancelled = true;
    };
  }, [diagram.background, diagram.source]);

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (!selectedNodeId || event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }
      let dx = 0;
      let dy = 0;
      if (event.key === "ArrowUp") {
        dy = -NUDGE_PIXELS;
      } else if (event.key === "ArrowDown") {
        dy = NUDGE_PIXELS;
      } else if (event.key === "ArrowLeft") {
        dx = -NUDGE_PIXELS;
      } else if (event.key === "ArrowRight") {
        dx = NUDGE_PIXELS;
      } else {
        return;
      }
      const core = coreRef.current;
      if (!core) {
        return;
      }
      try {
        const patch = core.nudgeNode(selectedNodeId, dx, dy);
        applyLayoutPatch(patch, onLayoutUpdate, onNodeMove, onEdgeMove);
        renderFromCore();
        event.preventDefault();
      } catch (err) {
        const message = err instanceof Error ? err.message : "Failed to nudge node";
        setError(message);
      }
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [onEdgeMove, onLayoutUpdate, onNodeMove, renderFromCore, selectedNodeId]);

  const handlePointerDown = (event: ReactPointerEvent<HTMLDivElement>) => {
    const core = coreRef.current;
    if (!core) {
      return;
    }

    const svg = wrapperRef.current?.querySelector("svg");
    if (!svg) {
      return;
    }

    const point = toDiagramPoint(svg, event.clientX, event.clientY);
    if (!point) {
      return;
    }

    const target = event.target as Element;
    const ganttTaskGroup = target.closest("g.gantt-task[data-task-id]");
    const subgraphGroup = target.closest("g.subgraph[data-id]");
    const nodeGroup = target.closest("g.node[data-id]");
    const edgeGroup = target.closest("g.edge[data-id]");

    if (ganttTaskGroup) {
      const taskId = ganttTaskGroup.getAttribute("data-task-id");
      if (!taskId) {
        return;
      }
      onSelectEdge(null);
      onSelectNode(taskId);
      const handle = target.closest(".gantt-handle[data-drag-kind]");
      const mode = handle?.getAttribute("data-drag-kind") ?? "move";
      try {
        core.beginGanttTaskDrag(taskId, mode, point.x);
        dragRef.current = { kind: "gantt", id: taskId, pointerId: event.pointerId };
        onDragStateChange?.(true);
        (event.currentTarget as HTMLDivElement).setPointerCapture(event.pointerId);
        event.preventDefault();
      } catch (err) {
        const message = err instanceof Error ? err.message : "Failed to start gantt drag";
        setError(message);
      }
      return;
    }

    if (subgraphGroup) {
      const subgraphId = subgraphGroup.getAttribute("data-id");
      if (!subgraphId) {
        return;
      }
      onSelectNode(null);
      onSelectEdge(null);
      try {
        core.beginSubgraphDrag(subgraphId, point.x, point.y);
        dragRef.current = { kind: "subgraph", id: subgraphId, pointerId: event.pointerId };
        onDragStateChange?.(true);
        (event.currentTarget as HTMLDivElement).setPointerCapture(event.pointerId);
        event.preventDefault();
      } catch (err) {
        const message = err instanceof Error ? err.message : "Failed to start subgraph drag";
        setError(message);
      }
      return;
    }

    if (nodeGroup) {
      const nodeId = nodeGroup.getAttribute("data-id");
      if (!nodeId) {
        return;
      }
      onSelectEdge(null);
      if (selectedNodeId !== nodeId) {
        onSelectNode(nodeId);
        return;
      }

      try {
        core.beginNodeDrag(nodeId, point.x, point.y);
        dragRef.current = { kind: "node", id: nodeId, pointerId: event.pointerId };
        onDragStateChange?.(true);
        (event.currentTarget as HTMLDivElement).setPointerCapture(event.pointerId);
        event.preventDefault();
      } catch (err) {
        const message = err instanceof Error ? err.message : "Failed to start node drag";
        setError(message);
      }
      return;
    }

    if (edgeGroup) {
      const edgeId = edgeGroup.getAttribute("data-id");
      if (!edgeId) {
        return;
      }
      onSelectNode(null);
      if (selectedEdgeId !== edgeId) {
        onSelectEdge(edgeId);
        return;
      }
      try {
        const index = pickEdgeHandleIndex(core, edgeId, point);
        core.beginEdgeDrag(edgeId, index);
        core.updateEdgeDrag(point.x, point.y);
        dragRef.current = { kind: "edge", id: edgeId, pointerId: event.pointerId };
        onDragStateChange?.(true);
        (event.currentTarget as HTMLDivElement).setPointerCapture(event.pointerId);
        event.preventDefault();
      } catch (err) {
        const message = err instanceof Error ? err.message : "Failed to start edge drag";
        setError(message);
      }
      return;
    }

    onSelectNode(null);
    onSelectEdge(null);
  };

  const handlePointerMove = (event: ReactPointerEvent<HTMLDivElement>) => {
    const core = coreRef.current;
    const active = dragRef.current;
    if (!core || !active || active.pointerId !== event.pointerId) {
      return;
    }

    const svg = wrapperRef.current?.querySelector("svg");
    if (!svg) {
      return;
    }

    const point = toDiagramPoint(svg, event.clientX, event.clientY);
    if (!point) {
      return;
    }

    try {
      if (active.kind === "node") {
        core.updateNodeDrag(point.x, point.y);
      } else if (active.kind === "edge") {
        core.updateEdgeDrag(point.x, point.y);
      } else if (active.kind === "subgraph") {
        core.updateSubgraphDrag(point.x, point.y);
      } else {
        core.updateGanttTaskDrag(point.x);
      }
      renderFromCore();
      event.preventDefault();
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to update drag";
      setError(message);
    }
  };

  const finishDrag = (pointerId: number) => {
    const core = coreRef.current;
    const active = dragRef.current;
    if (!core || !active || active.pointerId !== pointerId) {
      return;
    }

    try {
      let patch: unknown = null;
      if (active.kind === "node") {
        patch = core.endNodeDrag();
      } else if (active.kind === "edge") {
        patch = core.endEdgeDrag();
      } else if (active.kind === "subgraph") {
        patch = core.endSubgraphDrag();
      } else {
        patch = core.endGanttTaskDrag();
      }
      applyLayoutPatch(patch, onLayoutUpdate, onNodeMove, onEdgeMove);
      renderFromCore();
    } catch (err) {
      const message = err instanceof Error ? err.message : "Failed to finish drag";
      setError(message);
      core.cancelDrag();
    } finally {
      dragRef.current = null;
      onDragStateChange?.(false);
    }
  };

  const handlePointerUp = (event: ReactPointerEvent<HTMLDivElement>) => {
    finishDrag(event.pointerId);
    if ((event.currentTarget as HTMLDivElement).hasPointerCapture(event.pointerId)) {
      (event.currentTarget as HTMLDivElement).releasePointerCapture(event.pointerId);
    }
  };

  const handlePointerCancel = (event: ReactPointerEvent<HTMLDivElement>) => {
    const core = coreRef.current;
    if (core) {
      core.cancelDrag();
      renderFromCore();
    }
    dragRef.current = null;
    onDragStateChange?.(false);
    if ((event.currentTarget as HTMLDivElement).hasPointerCapture(event.pointerId)) {
      (event.currentTarget as HTMLDivElement).releasePointerCapture(event.pointerId);
    }
  };

  const handleWheel = (event: ReactWheelEvent<HTMLDivElement>) => {
    const svg = wrapperRef.current?.querySelector("svg");
    if (!svg) {
      return;
    }
    if (event.ctrlKey || event.metaKey) {
      const factor = event.deltaY < 0 ? ZOOM_STEP : 1 / ZOOM_STEP;
      setTransform((prev) => ({
        ...prev,
        scale: Math.min(ZOOM_MAX, Math.max(ZOOM_MIN, prev.scale * factor)),
      }));
      event.preventDefault();
      return;
    }
    setTransform((prev) => ({
      ...prev,
      x: prev.x - event.deltaX,
      y: prev.y - event.deltaY,
    }));
    event.preventDefault();
  };

  if (error) {
    return (
      <div className="diagram-canvas">
        <div className="placeholder">{error}</div>
      </div>
    );
  }

  return (
    <div
      ref={wrapperRef}
      className="diagram-canvas"
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerCancel}
      onWheel={handleWheel}
    >
      <div
        style={{
          transform: `translate(${transform.x}px, ${transform.y}px) scale(${transform.scale})`,
          transformOrigin: "0 0",
          width: "fit-content",
          height: "fit-content",
        }}
        dangerouslySetInnerHTML={{ __html: svgMarkup }}
      />
      <div className="zoom-controls">
        <button type="button" onClick={zoomOut} title="Zoom out">-</button>
        <button type="button" className="zoom-display" onClick={resetZoom} title="Reset zoom">
          {Math.round(transform.scale * 100)}%
        </button>
        <button type="button" onClick={zoomIn} title="Zoom in">+</button>
      </div>
    </div>
  );
}
