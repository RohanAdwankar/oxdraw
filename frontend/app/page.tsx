'use client';

import { ChangeEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import DiagramCanvas from "../components/DiagramCanvas";
import {
  deleteEdge,
  deleteNode,
  fetchDiagram,
  updateLayout,
  updateSource,
  updateStyle,
} from "../lib/api";
import {
  DiagramData,
  EdgeArrowDirection,
  EdgeKind,
  LayoutUpdate,
  EdgeStyleUpdate,
  NodeStyleUpdate,
  NodeData,
  Point,
} from "../lib/types";

function hasOverrides(diagram: DiagramData | null): boolean {
  if (!diagram) {
    return false;
  }
  return (
    diagram.nodes.some((node) => node.overridePosition) ||
    diagram.edges.some((edge) => edge.overridePoints && edge.overridePoints.length > 0)
  );
}

const DEFAULT_NODE_COLORS: Record<NodeData["shape"], string> = {
  rectangle: "#FDE68A",
  stadium: "#C4F1F9",
  circle: "#E9D8FD",
  diamond: "#FBCFE8",
};

const DEFAULT_EDGE_COLOR = "#2d3748";
const DEFAULT_NODE_TEXT = "#1a202c";

const LINE_STYLE_OPTIONS: Array<{ value: EdgeKind; label: string }> = [
  { value: "solid", label: "Solid" },
  { value: "dashed", label: "Dashed" },
];

const ARROW_DIRECTION_OPTIONS: Array<{ value: EdgeArrowDirection; label: string }> = [
  { value: "forward", label: "Forward" },
  { value: "backward", label: "Backward" },
  { value: "both", label: "Both" },
  { value: "none", label: "None" },
];

const HEX_COLOR_RE = /^#([0-9a-f]{6})$/i;

const resolveColor = (value: string | null | undefined, fallback: string): string => {
  const base = value ?? fallback;
  if (HEX_COLOR_RE.test(base)) {
    return base.toLowerCase();
  }
  if (HEX_COLOR_RE.test(fallback)) {
    return fallback.toLowerCase();
  }
  return "#000000";
};

const normalizeColorInput = (value: string): string => value.trim().toLowerCase();

export default function Home() {
  const [diagram, setDiagram] = useState<DiagramData | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const [source, setSource] = useState("");
  const [sourceDraft, setSourceDraft] = useState("");
  const [sourceSaving, setSourceSaving] = useState(false);
  const [sourceError, setSourceError] = useState<string | null>(null);
  const [selectedNodeId, setSelectedNodeId] = useState<string | null>(null);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const [dragging, setDragging] = useState(false);
  const saveTimer = useRef<number | null>(null);
  const lastSubmittedSource = useRef<string | null>(null);

  const selectedNode = useMemo(() => {
    if (!diagram || !selectedNodeId) {
      return null;
    }
    return diagram.nodes.find((node) => node.id === selectedNodeId) ?? null;
  }, [diagram, selectedNodeId]);

  const selectedEdge = useMemo(() => {
    if (!diagram || !selectedEdgeId) {
      return null;
    }
    return diagram.edges.find((edge) => edge.id === selectedEdgeId) ?? null;
  }, [diagram, selectedEdgeId]);

  const loadDiagram = useCallback(
    async (options?: { silent?: boolean }) => {
      const silent = options?.silent ?? false;
      try {
        if (!silent) {
          setLoading(true);
        }
        setError(null);
  const data = await fetchDiagram();
  setDiagram(data);
  setSource(data.source);
  setSourceDraft(data.source);
  lastSubmittedSource.current = data.source;
        setSourceError(null);
        setSourceSaving(false);
        setSelectedNodeId((current) =>
          current && data.nodes.some((node) => node.id === current) ? current : null
        );
        setSelectedEdgeId((current) =>
          current && data.edges.some((edge) => edge.id === current) ? current : null
        );
        return data;
      } catch (err) {
        setError((err as Error).message);
        if (!silent) {
          setDiagram(null);
        }
        throw err;
      } finally {
        if (!silent) {
          setLoading(false);
        }
      }
    },
    []
  );

  useEffect(() => {
    void loadDiagram().catch(() => undefined);
  }, [loadDiagram]);

  const applyUpdate = useCallback(
    async (update: LayoutUpdate) => {
      try {
        setSaving(true);
        await updateLayout(update);
        await loadDiagram({ silent: true });
      } catch (err) {
        setError((err as Error).message);
      } finally {
        setSaving(false);
      }
    },
    [loadDiagram]
  );

  const submitStyleUpdate = useCallback(
    async (update: {
      nodeStyles?: Record<string, NodeStyleUpdate | null>;
      edgeStyles?: Record<string, EdgeStyleUpdate | null>;
    }) => {
      const hasNodeStyles = update.nodeStyles && Object.keys(update.nodeStyles).length > 0;
      const hasEdgeStyles = update.edgeStyles && Object.keys(update.edgeStyles).length > 0;
      if (!hasNodeStyles && !hasEdgeStyles) {
        return;
      }

      try {
        setSaving(true);
        setError(null);
        await updateStyle({
          nodeStyles: update.nodeStyles,
          edgeStyles: update.edgeStyles,
        });
        await loadDiagram({ silent: true });
      } catch (err) {
        setError((err as Error).message);
      } finally {
        setSaving(false);
      }
    },
    [loadDiagram]
  );

  const handleNodeFillChange = useCallback(
    (value: string) => {
      if (!selectedNode) {
        return;
      }
      const normalized = normalizeColorInput(value);
      const fallback = DEFAULT_NODE_COLORS[selectedNode.shape];
      const currentFill = resolveColor(selectedNode.fillColor, fallback);
      if (currentFill === normalized) {
        return;
      }
      void submitStyleUpdate({
        nodeStyles: {
          [selectedNode.id]: {
            fill: normalized,
          },
        },
      });
    },
    [selectedNode, submitStyleUpdate]
  );

  const handleNodeStrokeChange = useCallback(
    (value: string) => {
      if (!selectedNode) {
        return;
      }
      const normalized = normalizeColorInput(value);
      const currentStroke = resolveColor(selectedNode.strokeColor, DEFAULT_EDGE_COLOR);
      if (currentStroke === normalized) {
        return;
      }
      void submitStyleUpdate({
        nodeStyles: {
          [selectedNode.id]: {
            stroke: normalized,
          },
        },
      });
    },
    [selectedNode, submitStyleUpdate]
  );

  const handleNodeTextColorChange = useCallback(
    (value: string) => {
      if (!selectedNode) {
        return;
      }
      const normalized = normalizeColorInput(value);
      const currentText = resolveColor(selectedNode.textColor, DEFAULT_NODE_TEXT);
      if (currentText === normalized) {
        return;
      }
      void submitStyleUpdate({
        nodeStyles: {
          [selectedNode.id]: {
            text: normalized,
          },
        },
      });
    },
    [selectedNode, submitStyleUpdate]
  );

  const handleNodeStyleReset = useCallback(() => {
    if (!selectedNode) {
      return;
    }
    void submitStyleUpdate({
      nodeStyles: {
        [selectedNode.id]: null,
      },
    });
  }, [selectedNode, submitStyleUpdate]);

  const handleEdgeColorChange = useCallback(
    (value: string) => {
      if (!selectedEdge) {
        return;
      }
      const normalized = normalizeColorInput(value);
      const currentColor = resolveColor(selectedEdge.color, DEFAULT_EDGE_COLOR);
      if (currentColor === normalized) {
        return;
      }
      void submitStyleUpdate({
        edgeStyles: {
          [selectedEdge.id]: {
            color: normalized,
          },
        },
      });
    },
    [selectedEdge, submitStyleUpdate]
  );

  const handleEdgeLineStyleChange = useCallback(
    (value: EdgeKind) => {
      if (!selectedEdge) {
        return;
      }
      if (selectedEdge.kind === value) {
        return;
      }
      void submitStyleUpdate({
        edgeStyles: {
          [selectedEdge.id]: {
            line: value,
          },
        },
      });
    },
    [selectedEdge, submitStyleUpdate]
  );

  const handleEdgeArrowChange = useCallback(
    (value: EdgeArrowDirection) => {
      if (!selectedEdge) {
        return;
      }
      const currentArrow = selectedEdge.arrowDirection ?? "forward";
      if (currentArrow === value) {
        return;
      }
      void submitStyleUpdate({
        edgeStyles: {
          [selectedEdge.id]: {
            arrow: value,
          },
        },
      });
    },
    [selectedEdge, submitStyleUpdate]
  );

  const handleEdgeStyleReset = useCallback(() => {
    if (!selectedEdge) {
      return;
    }
    void submitStyleUpdate({
      edgeStyles: {
        [selectedEdge.id]: null,
      },
    });
  }, [selectedEdge, submitStyleUpdate]);

  const handleAddEdgeJoint = useCallback(() => {
    if (!selectedEdge) {
      return;
    }

    const route = selectedEdge.renderedPoints;
    if (route.length < 2) {
      return;
    }

    let bestSegment = 0;
    let bestLength = -Infinity;
    for (let index = 0; index < route.length - 1; index += 1) {
      const start = route[index];
      const end = route[index + 1];
      const length = Math.hypot(end.x - start.x, end.y - start.y);
      if (length > bestLength) {
        bestLength = length;
        bestSegment = index;
      }
    }

    const start = route[bestSegment];
    const end = route[bestSegment + 1];
    const newPoint: Point = {
      x: (start.x + end.x) / 2,
      y: (start.y + end.y) / 2,
    };

    const currentOverrides = selectedEdge.overridePoints
      ? selectedEdge.overridePoints.map((point) => ({ ...point }))
      : [];

    const alreadyPresent = currentOverrides.some((point) => {
      const dx = point.x - newPoint.x;
      const dy = point.y - newPoint.y;
      return Math.hypot(dx, dy) < 0.25;
    });
    if (alreadyPresent) {
      return;
    }

    const insertIndex = Math.min(bestSegment, currentOverrides.length);
    currentOverrides.splice(insertIndex, 0, newPoint);

    void applyUpdate({
      edges: {
        [selectedEdge.id]: {
          points: currentOverrides,
        },
      },
    });
  }, [applyUpdate, selectedEdge]);

  const handleNodeMove = useCallback(
    (id: string, position: Point | null) => {
      void applyUpdate({
        nodes: {
          [id]: position,
        },
      });
    },
    [applyUpdate]
  );

  const handleEdgeMove = useCallback(
    (id: string, points: Point[] | null) => {
      void applyUpdate({
        edges: {
          [id]: {
            points,
          },
        },
      });
    },
    [applyUpdate]
  );

  const handleSourceChange = useCallback((event: ChangeEvent<HTMLTextAreaElement>) => {
    const value = event.target.value;
    lastSubmittedSource.current = null;
    setSourceDraft(value);
    setError(null);
    setSourceError(null);
  }, []);

  const handleSelectNode = useCallback((id: string | null) => {
    setSelectedNodeId(id);
    if (id) {
      setSelectedEdgeId(null);
    }
  }, []);

  const handleSelectEdge = useCallback((id: string | null) => {
    setSelectedEdgeId(id);
    if (id) {
      setSelectedNodeId(null);
    }
  }, []);

  const deleteTarget = useCallback(
    async (target: { type: "node" | "edge"; id: string }) => {
      if (saving || sourceSaving) {
        return;
      }
      try {
        setSaving(true);
        setError(null);
        if (target.type === "node") {
          await deleteNode(target.id);
          setSelectedNodeId((current) => (current === target.id ? null : current));
          setSelectedEdgeId(null);
        } else {
          await deleteEdge(target.id);
          setSelectedEdgeId((current) => (current === target.id ? null : current));
        }
        await loadDiagram({ silent: true });
      } catch (err) {
        setError((err as Error).message);
      } finally {
        setSaving(false);
      }
    },
    [deleteEdge, deleteNode, loadDiagram, saving, sourceSaving]
  );

  const handleDeleteSelection = useCallback(async () => {
    if (selectedNodeId) {
      await deleteTarget({ type: "node", id: selectedNodeId });
    } else if (selectedEdgeId) {
      await deleteTarget({ type: "edge", id: selectedEdgeId });
    }
  }, [deleteTarget, selectedEdgeId, selectedNodeId]);

  const handleDeleteNodeDirect = useCallback(
    async (id: string) => {
      await deleteTarget({ type: "node", id });
    },
    [deleteTarget]
  );

  const handleDeleteEdgeDirect = useCallback(
    async (id: string) => {
      await deleteTarget({ type: "edge", id });
    },
    [deleteTarget]
  );

  const handleResetOverrides = useCallback(() => {
    if (!diagram) {
      return;
    }

    const nodesUpdate: Record<string, Point | null> = {};
    const edgesUpdate: Record<string, { points?: Point[] | null }> = {};

    for (const node of diagram.nodes) {
      if (node.overridePosition) {
        nodesUpdate[node.id] = null;
      }
    }

    for (const edge of diagram.edges) {
      if (edge.overridePoints && edge.overridePoints.length > 0) {
        edgesUpdate[edge.id] = { points: null };
      }
    }

    if (Object.keys(nodesUpdate).length === 0 && Object.keys(edgesUpdate).length === 0) {
      return;
    }

    void applyUpdate({ nodes: nodesUpdate, edges: edgesUpdate });
  }, [applyUpdate, diagram]);

  const statusMessage = useMemo(() => {
    if (loading) {
      return "Loading diagram...";
    }
    if (saving) {
      return "Saving changes...";
    }
    if (sourceSaving) {
      return "Syncing source...";
    }
    if (error) {
      return `Error: ${error}`;
    }
    return diagram ? `Editing ${diagram.sourcePath}` : "No diagram selected";
  }, [diagram, error, loading, saving, sourceSaving]);

  useEffect(() => {
    if (!diagram || dragging) {
      if (saveTimer.current !== null) {
        window.clearTimeout(saveTimer.current);
        saveTimer.current = null;
      }
      return;
    }

    if (saveTimer.current !== null) {
      window.clearTimeout(saveTimer.current);
      saveTimer.current = null;
    }

    if (sourceDraft === source) {
      setSourceSaving(false);
      if (sourceError) {
        setSourceError(null);
      }
      lastSubmittedSource.current = sourceDraft;
      return;
    }

    if (lastSubmittedSource.current === sourceDraft && sourceError) {
      return;
    }

    setSourceSaving(true);
    saveTimer.current = window.setTimeout(() => {
      const payload = sourceDraft;
      lastSubmittedSource.current = payload;
      void (async () => {
        try {
          await updateSource(payload);
          setSourceSaving(false);
          setSourceError(null);
          await loadDiagram({ silent: true });
        } catch (err) {
          const message = (err as Error).message;
          setSourceSaving(false);
          setSourceError(message);
          setError(message);
        }
      })();
    }, 700);

    return () => {
      if (saveTimer.current !== null) {
        window.clearTimeout(saveTimer.current);
        saveTimer.current = null;
      }
    };
  }, [diagram, dragging, sourceDraft, source, sourceError, loadDiagram]);

  const sourceStatus = useMemo(() => {
    if (sourceError) {
      return { label: sourceError, variant: "error" as const };
    }
    if (sourceSaving) {
      return { label: "Saving changes…", variant: "saving" as const };
    }
    if (sourceDraft !== source) {
      return { label: "Pending changes…", variant: "pending" as const };
    }
    return { label: "Synced", variant: "synced" as const };
  }, [sourceError, sourceSaving, sourceDraft, source]);

  const selectionLabel = useMemo(() => {
    if (selectedNodeId) {
      return `Selected node: ${selectedNodeId}`;
    }
    if (selectedEdgeId) {
      return `Selected edge: ${selectedEdgeId}`;
    }
    return "No selection";
  }, [selectedEdgeId, selectedNodeId]);

  const hasSelection = selectedNodeId !== null || selectedEdgeId !== null;

  const nodeFillValue = useMemo(() => {
    if (!selectedNode) {
      return DEFAULT_NODE_COLORS.rectangle.toLowerCase();
    }
    return resolveColor(selectedNode.fillColor, DEFAULT_NODE_COLORS[selectedNode.shape]);
  }, [selectedNode]);

  const nodeStrokeValue = useMemo(() => {
    if (!selectedNode) {
      return DEFAULT_EDGE_COLOR.toLowerCase();
    }
    return resolveColor(selectedNode.strokeColor, DEFAULT_EDGE_COLOR);
  }, [selectedNode]);

  const nodeTextValue = useMemo(() => {
    if (!selectedNode) {
      return DEFAULT_NODE_TEXT.toLowerCase();
    }
    return resolveColor(selectedNode.textColor, DEFAULT_NODE_TEXT);
  }, [selectedNode]);

  const edgeColorValue = useMemo(() => {
    if (!selectedEdge) {
      return DEFAULT_EDGE_COLOR.toLowerCase();
    }
    return resolveColor(selectedEdge.color, DEFAULT_EDGE_COLOR);
  }, [selectedEdge]);

  const edgeLineValue = selectedEdge?.kind ?? "solid";
  const edgeArrowValue = selectedEdge?.arrowDirection ?? "forward";

  const nodeControlsDisabled = !selectedNode || saving;
  const edgeControlsDisabled = !selectedEdge || saving;

  useEffect(() => {
    const handleKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "Delete" && event.key !== "Backspace") {
        return;
      }
      const active = document.activeElement as HTMLElement | null;
      if (
        active &&
        (active.tagName === "TEXTAREA" || active.tagName === "INPUT" || active.isContentEditable)
      ) {
        return;
      }
      if (!selectedNodeId && !selectedEdgeId) {
        return;
      }
      event.preventDefault();
      void handleDeleteSelection();
    };

    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleDeleteSelection, selectedEdgeId, selectedNodeId]);

  return (
    <div className="app">
      <header className="toolbar">
        <div className="status" role="status" aria-live="polite">
          {statusMessage}
        </div>
        <div className="actions">
          <button
            onClick={handleResetOverrides}
            disabled={!hasOverrides(diagram) || saving || sourceSaving}
            title="Remove all manual positions"
          >
            Reset overrides
          </button>
          <button
            onClick={() => void handleDeleteSelection()}
            disabled={!hasSelection || saving || sourceSaving}
            title="Delete the currently selected node or edge"
          >
            Delete selected
          </button>
        </div>
      </header>
      <main className="workspace">
        {diagram && !loading ? (
          <>
            <aside className="style-panel">
              <div className="panel-header">
                <span className="panel-title">Style</span>
                <span className="panel-caption">
                  {selectedNode
                    ? `Node: ${selectedNode.label || selectedNode.id}`
                    : selectedEdge
                      ? `Edge: ${selectedEdge.label || `${selectedEdge.from}→${selectedEdge.to}`}`
                      : "Select an element"}
                </span>
              </div>
              <div className="panel-body">
                <section className="style-section">
                  <header className="section-heading">
                    <h3>Node</h3>
                    <span className={selectedNode ? "section-caption" : "section-caption muted"}>
                      {selectedNode ? selectedNode.label || selectedNode.id : "No node selected"}
                    </span>
                  </header>
                  <div className="style-controls" aria-disabled={nodeControlsDisabled}>
                    <label className="style-control">
                      <span>Fill</span>
                      <input
                        type="color"
                        value={nodeFillValue}
                        onChange={(event) => handleNodeFillChange(event.target.value)}
                        disabled={nodeControlsDisabled}
                      />
                    </label>
                    <label className="style-control">
                      <span>Stroke</span>
                      <input
                        type="color"
                        value={nodeStrokeValue}
                        onChange={(event) => handleNodeStrokeChange(event.target.value)}
                        disabled={nodeControlsDisabled}
                      />
                    </label>
                    <label className="style-control">
                      <span>Text</span>
                      <input
                        type="color"
                        value={nodeTextValue}
                        onChange={(event) => handleNodeTextColorChange(event.target.value)}
                        disabled={nodeControlsDisabled}
                      />
                    </label>
                  </div>
                  <button
                    type="button"
                    className="style-reset"
                    onClick={() => void handleNodeStyleReset()}
                    disabled={nodeControlsDisabled}
                  >
                    Reset node style
                  </button>
                </section>

                <section className="style-section">
                  <header className="section-heading">
                    <h3>Edge</h3>
                    <span className={selectedEdge ? "section-caption" : "section-caption muted"}>
                      {selectedEdge
                        ? selectedEdge.label || `${selectedEdge.from}→${selectedEdge.to}`
                        : "No edge selected"}
                    </span>
                  </header>
                  <div className="style-controls" aria-disabled={edgeControlsDisabled}>
                    <label className="style-control">
                      <span>Color</span>
                      <input
                        type="color"
                        value={edgeColorValue}
                        onChange={(event) => handleEdgeColorChange(event.target.value)}
                        disabled={edgeControlsDisabled}
                      />
                    </label>
                    <label className="style-control">
                      <span>Line</span>
                      <select
                        value={edgeLineValue}
                        onChange={(event) => handleEdgeLineStyleChange(event.target.value as EdgeKind)}
                        disabled={edgeControlsDisabled}
                      >
                        {LINE_STYLE_OPTIONS.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                    </label>
                    <label className="style-control">
                      <span>Arrows</span>
                      <select
                        value={edgeArrowValue}
                        onChange={(event) => handleEdgeArrowChange(event.target.value as EdgeArrowDirection)}
                        disabled={edgeControlsDisabled}
                      >
                        {ARROW_DIRECTION_OPTIONS.map((option) => (
                          <option key={option.value} value={option.value}>
                            {option.label}
                          </option>
                        ))}
                      </select>
                    </label>
                  </div>
                  <button
                    type="button"
                    className="style-reset"
                    onClick={handleAddEdgeJoint}
                    disabled={edgeControlsDisabled}
                  >
                    Add control point
                  </button>
                  <button
                    type="button"
                    className="style-reset"
                    onClick={() => void handleEdgeStyleReset()}
                    disabled={edgeControlsDisabled}
                  >
                    Reset edge style
                  </button>
                </section>
              </div>
            </aside>
            <DiagramCanvas
              diagram={diagram}
              onNodeMove={handleNodeMove}
              onEdgeMove={handleEdgeMove}
              selectedNodeId={selectedNodeId}
              selectedEdgeId={selectedEdgeId}
              onSelectNode={handleSelectNode}
              onSelectEdge={handleSelectEdge}
              onDragStateChange={setDragging}
              onDeleteNode={handleDeleteNodeDirect}
              onDeleteEdge={handleDeleteEdgeDirect}
            />
            <aside className="source-panel">
              <div className="panel-header">
                <span className="panel-title">Source</span>
                <span className="panel-path">{diagram.sourcePath}</span>
              </div>
              <textarea
                className="source-editor"
                value={sourceDraft}
                onChange={handleSourceChange}
                spellCheck={false}
                aria-label="Diagram source"
              />
              <div className="panel-footer">
                <span className={`source-status ${sourceStatus.variant}`}>{sourceStatus.label}</span>
                <span className="selection-label">{selectionLabel}</span>
              </div>
            </aside>
          </>
        ) : (
          <div className="placeholder">{loading ? "Loading…" : "No diagram"}</div>
        )}
      </main>
      {error && (
        <footer className="error" role="alert">
          {error}
        </footer>
      )}
    </div>
  );
}
