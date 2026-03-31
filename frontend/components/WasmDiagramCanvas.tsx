'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
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

interface MarqueeSelection {
  pointerId: number;
  origin: Point;
  current: Point;
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
  selectedNodeIds,
  selectedEdgeId,
  onSelectNode,
  onSelectNodes,
  onSelectEdge,
  onDragStateChange,
  activeTool,
  connectStartNodeId,
  addNodeSourceId,
  onCanvasAddNode,
  onAddNodeSourceSelect,
  onConnectNodeClick,
  onContextMenuRequest,
  onViewportControlsChange,
}: DiagramCanvasProps) {
  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const [svgMarkup, setSvgMarkup] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [transform, setTransform] = useState({ x: 0, y: 0, scale: 1 });
  const coreRef = useRef<WasmEditorCore | null>(null);
  const dragRef = useRef<ActiveDrag | null>(null);
  const marqueeRef = useRef<MarqueeSelection | null>(null);
  const [marqueeRect, setMarqueeRect] = useState<{ x: number; y: number; width: number; height: number } | null>(null);

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
    onViewportControlsChange?.({
      zoomIn,
      zoomOut,
      resetZoom,
      getScale: () => transform.scale,
    });
    return () => onViewportControlsChange?.(null);
  }, [onViewportControlsChange, resetZoom, transform.scale, zoomIn, zoomOut]);

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
    const root = wrapperRef.current;
    if (!root) {
      return;
    }
    const nodeIds = new Set(selectedNodeIds);
    root.querySelectorAll("g.node[data-id]").forEach((element) => {
      const id = element.getAttribute("data-id");
      element.classList.toggle("selected", !!id && nodeIds.has(id));
    });
    root.querySelectorAll("g.edge[data-id]").forEach((element) => {
      const id = element.getAttribute("data-id");
      element.classList.toggle("selected", !!id && id === selectedEdgeId);
    });
  }, [selectedEdgeId, selectedNodeIds, svgMarkup]);

  const nodeBounds = useMemo(
    () =>
      diagram.nodes.map((node) => {
        const center = node.renderedPosition;
        return {
          id: node.id,
          left: center.x - node.width / 2,
          right: center.x + node.width / 2,
          top: center.y - node.height / 2,
          bottom: center.y + node.height / 2,
        };
      }),
    [diagram.nodes]
  );

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
      if (nodeGroup) {
        const nodeId = nodeGroup.getAttribute("data-id");
        if (nodeId) {
          onSelectEdge(null);
          onSelectNode(nodeId);
          onSelectNodes([nodeId]);
          onAddNodeSourceSelect(nodeId);
          event.preventDefault();
        }
        return;
      }
      if (!ganttTaskGroup && !subgraphGroup && !edgeGroup) {
        onSelectNode(null);
        onSelectNodes([]);
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

    if (!ganttTaskGroup && !subgraphGroup && !nodeGroup && !edgeGroup && activeTool === "select") {
      marqueeRef.current = {
        pointerId: event.pointerId,
        origin: point,
        current: point,
      };
      setMarqueeRect({ x: point.x, y: point.y, width: 0, height: 0 });
      onSelectNode(null);
      onSelectNodes([]);
      onSelectEdge(null);
      onDragStateChange?.(true);
      (event.currentTarget as HTMLDivElement).setPointerCapture(event.pointerId);
      event.preventDefault();
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
      onSelectNodes([nodeId]);
      onAddNodeSourceSelect(null);

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
    const marquee = marqueeRef.current;
    if (marquee && marquee.pointerId === event.pointerId) {
      const svg = wrapperRef.current?.querySelector("svg");
      if (!svg) {
        return;
      }
      const point = toDiagramPoint(svg, event.clientX, event.clientY);
      if (!point) {
        return;
      }
      marquee.current = point;
      const x = Math.min(marquee.origin.x, point.x);
      const y = Math.min(marquee.origin.y, point.y);
      const width = Math.abs(point.x - marquee.origin.x);
      const height = Math.abs(point.y - marquee.origin.y);
      setMarqueeRect({ x, y, width, height });
      event.preventDefault();
      return;
    }

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
    const marquee = marqueeRef.current;
    if (marquee && marquee.pointerId === event.pointerId) {
      const current = marquee.current;
      const x = Math.min(marquee.origin.x, current.x);
      const y = Math.min(marquee.origin.y, current.y);
      const right = Math.max(marquee.origin.x, current.x);
      const bottom = Math.max(marquee.origin.y, current.y);
      const selectedIds = nodeBounds
        .filter(
          (node) =>
            node.right >= x &&
            node.left <= right &&
            node.bottom >= y &&
            node.top <= bottom
        )
        .map((node) => node.id);
      onSelectNodes(selectedIds);
      onSelectNode(selectedIds.length === 1 ? selectedIds[0] : null);
      onSelectEdge(null);
      marqueeRef.current = null;
      setMarqueeRect(null);
      onDragStateChange?.(false);
      if ((event.currentTarget as HTMLDivElement).hasPointerCapture(event.pointerId)) {
        (event.currentTarget as HTMLDivElement).releasePointerCapture(event.pointerId);
      }
      event.preventDefault();
      return;
    }
    finishDrag(event.pointerId);
    if ((event.currentTarget as HTMLDivElement).hasPointerCapture(event.pointerId)) {
      (event.currentTarget as HTMLDivElement).releasePointerCapture(event.pointerId);
    }
  };

  const handlePointerCancel = (event: ReactPointerEvent<HTMLDivElement>) => {
    marqueeRef.current = null;
    setMarqueeRect(null);
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
      {marqueeRect ? (
        <div
          className="marquee-selection"
          style={{
            left: marqueeRect.x * transform.scale + transform.x,
            top: marqueeRect.y * transform.scale + transform.y,
            width: marqueeRect.width * transform.scale,
            height: marqueeRect.height * transform.scale,
          }}
        />
      ) : null}
      {activeTool === "connect" ? (
        <div className="canvas-mode-badge">
          {connectStartNodeId ? `Connect: ${connectStartNodeId} ->` : "Connect mode"}
        </div>
      ) : null}
      {activeTool === "add-node" ? (
        <div className="canvas-mode-badge">
          {addNodeSourceId ? `Add from: ${addNodeSourceId}` : "Click canvas to add node"}
        </div>
      ) : null}
    </div>
  );
}
