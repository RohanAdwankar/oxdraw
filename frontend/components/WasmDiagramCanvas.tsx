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

type UnknownEntryMap<T> = Record<string, T> | Map<string, T> | null | undefined;

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

  const mapEntries = <T,>(value: UnknownEntryMap<T>): Array<[string, T]> => {
    if (!value) {
      return [];
    }
    if (value instanceof Map) {
      return Array.from(value.entries());
    }
    return Object.entries(value);
  };

  const payload = patch as {
    nodes?: UnknownEntryMap<Point | null>;
    edges?: UnknownEntryMap<{ points?: Point[] | null } | null>;
    ganttTasks?: UnknownEntryMap<{ startDay?: number; endDay?: number } | null>;
    gantt_tasks?: UnknownEntryMap<{ start_day?: number; end_day?: number } | null>;
  };

  const nodeEntries = mapEntries(payload.nodes);
  const edgeEntries = mapEntries(payload.edges);
  const ganttEntriesCamel = mapEntries(payload.ganttTasks);
  const ganttEntriesSnake = mapEntries(payload.gantt_tasks).map(([id, value]) => [
    id,
    value
      ? {
          startDay: value.start_day,
          endDay: value.end_day,
        }
      : null,
  ]) as Array<[string, { startDay?: number; endDay?: number } | null]>;
  const ganttEntries =
    ganttEntriesCamel.length > 0 ? ganttEntriesCamel : ganttEntriesSnake;

  const hasNodes = nodeEntries.length > 0;
  const hasEdges = edgeEntries.length > 0;
  const hasGanttTasks = ganttEntries.length > 0;

  if (!hasNodes && !hasEdges && !hasGanttTasks) {
    return;
  }

  if (onLayoutUpdate) {
    const update: LayoutUpdate = {};
    if (nodeEntries.length > 0) {
      update.nodes = Object.fromEntries(nodeEntries);
    }
    if (edgeEntries.length > 0) {
      const normalized: Record<string, { points?: Point[] | null }> = {};
      for (const [edgeId, value] of edgeEntries) {
        normalized[edgeId] = value ?? { points: null };
      }
      update.edges = normalized;
    }
    if (ganttEntries.length > 0) {
      update.ganttTasks = Object.fromEntries(ganttEntries);
    }
    onLayoutUpdate(update);
    return;
  }

  for (const [nodeId, value] of nodeEntries) {
    onNodeMove(nodeId, value);
  }
  for (const [edgeId, value] of edgeEntries) {
    onEdgeMove(edgeId, value?.points ?? null);
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
  onSvgMarkupChange,
  selectedNodeId,
  selectedEdgeId,
  onSelectNode,
  onSelectEdge,
  onDragStateChange,
  activeTool,
  connectStartNodeId,
  onCanvasAddNode,
  onConnectNodeClick,
  onContextMenuRequest,
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
    const nextMarkup = coreRef.current.renderSvg();
    setSvgMarkup(nextMarkup);
    onSvgMarkupChange?.(nextMarkup);
  }, [onSvgMarkupChange]);

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
        const nextMarkup = core.renderSvg();
        setSvgMarkup(nextMarkup);
        onSvgMarkupChange?.(nextMarkup);
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
  }, [diagram.background, diagram.source, onSvgMarkupChange]);

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
    if (event.button !== 0) {
      return;
    }

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

    if (activeTool === "add-node") {
      if (!ganttTaskGroup && !subgraphGroup && !nodeGroup && !edgeGroup) {
        onSelectNode(null);
        onSelectEdge(null);
        onCanvasAddNode(point);
        event.preventDefault();
      }
      return;
    }

    if (activeTool === "connect") {
      if (nodeGroup) {
        const nodeId = nodeGroup.getAttribute("data-id");
        if (nodeId) {
          onSelectEdge(null);
          onSelectNode(nodeId);
          onConnectNodeClick(nodeId);
          event.preventDefault();
        }
        return;
      }
      onSelectNode(null);
      onSelectEdge(null);
      return;
    }

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
      onSelectNode(nodeId);

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
      onSelectEdge(edgeId);
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

  const handleContextMenu = (event: React.MouseEvent<HTMLDivElement>) => {
    const svg = wrapperRef.current?.querySelector("svg");
    const point = svg ? toDiagramPoint(svg, event.clientX, event.clientY) : null;
    const target = event.target as Element;
    const nodeGroup = target.closest("g.node[data-id]");
    const edgeGroup = target.closest("g.edge[data-id]");

    if (nodeGroup) {
      const nodeId = nodeGroup.getAttribute("data-id");
      if (nodeId) {
        onSelectEdge(null);
        onSelectNode(nodeId);
        onContextMenuRequest?.({
          kind: "node",
          clientX: event.clientX,
          clientY: event.clientY,
          point,
          nodeId,
        });
        event.preventDefault();
        return;
      }
    }

    if (edgeGroup) {
      const edgeId = edgeGroup.getAttribute("data-id");
      if (edgeId) {
        onSelectNode(null);
        onSelectEdge(edgeId);
        onContextMenuRequest?.({
          kind: "edge",
          clientX: event.clientX,
          clientY: event.clientY,
          point,
          edgeId,
        });
        event.preventDefault();
        return;
      }
    }

    onContextMenuRequest?.({
      kind: "canvas",
      clientX: event.clientX,
      clientY: event.clientY,
      point,
    });
    event.preventDefault();
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
      className={`diagram-canvas tool-${activeTool}`}
      onPointerDown={handlePointerDown}
      onPointerMove={handlePointerMove}
      onPointerUp={handlePointerUp}
      onPointerCancel={handlePointerCancel}
      onWheel={handleWheel}
      onContextMenu={handleContextMenu}
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
      {activeTool === "connect" ? (
        <div className="canvas-mode-badge">
          {connectStartNodeId ? `Connect: ${connectStartNodeId} ->` : "Connect mode"}
        </div>
      ) : null}
      {activeTool === "add-node" ? (
        <div className="canvas-mode-badge">Click canvas to add node</div>
      ) : null}
    </div>
  );
}
