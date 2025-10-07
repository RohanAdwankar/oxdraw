'use client';

import { ChangeEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import DiagramCanvas from "../components/DiagramCanvas";
import { deleteEdge, deleteNode, fetchDiagram, updateLayout, updateSource } from "../lib/api";
import { DiagramData, LayoutUpdate, Point } from "../lib/types";

function hasOverrides(diagram: DiagramData | null): boolean {
  if (!diagram) {
    return false;
  }
  return (
    diagram.nodes.some((node) => node.overridePosition) ||
    diagram.edges.some((edge) => edge.overridePoints && edge.overridePoints.length > 0)
  );
}

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
  const saveTimer = useRef<number | null>(null);
  const lastSubmittedSource = useRef<string | null>(null);

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

  const handleDeleteSelection = useCallback(async () => {
    if (saving || sourceSaving) {
      return;
    }
    if (!selectedNodeId && !selectedEdgeId) {
      return;
    }
    try {
      setSaving(true);
      setError(null);
      if (selectedNodeId) {
        await deleteNode(selectedNodeId);
        setSelectedNodeId(null);
      } else if (selectedEdgeId) {
        await deleteEdge(selectedEdgeId);
        setSelectedEdgeId(null);
      }
      await loadDiagram({ silent: true });
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setSaving(false);
    }
  }, [deleteEdge, deleteNode, loadDiagram, saving, selectedEdgeId, selectedNodeId, sourceSaving]);

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
    if (!diagram) {
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
  }, [diagram, sourceDraft, source, sourceError, loadDiagram]);

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
          <button onClick={() => void loadDiagram().catch(() => undefined)} disabled={loading || saving}>
            Refresh
          </button>
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
            <DiagramCanvas
              diagram={diagram}
              onNodeMove={handleNodeMove}
              onEdgeMove={handleEdgeMove}
              selectedNodeId={selectedNodeId}
              selectedEdgeId={selectedEdgeId}
              onSelectNode={handleSelectNode}
              onSelectEdge={handleSelectEdge}
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
