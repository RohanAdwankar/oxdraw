'use client';

import { useCallback, useEffect, useMemo, useState } from "react";
import DiagramCanvas from "../components/DiagramCanvas";
import { fetchDiagram, updateLayout } from "../lib/api";
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

  const loadDiagram = useCallback(async () => {
    try {
      setLoading(true);
      setError(null);
      const data = await fetchDiagram();
      setDiagram(data);
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setLoading(false);
    }
  }, []);

  useEffect(() => {
    void loadDiagram();
  }, [loadDiagram]);

  const applyUpdate = useCallback(
    async (update: LayoutUpdate) => {
      try {
        setSaving(true);
        await updateLayout(update);
        await loadDiagram();
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
    if (error) {
      return `Error: ${error}`;
    }
    return diagram ? `Editing ${diagram.sourcePath}` : "No diagram selected";
  }, [diagram, error, loading, saving]);

  return (
    <div className="app">
      <header className="toolbar">
        <div className="status" role="status" aria-live="polite">
          {statusMessage}
        </div>
        <div className="actions">
          <button onClick={() => void loadDiagram()} disabled={loading || saving}>
            Refresh
          </button>
          <button
            onClick={handleResetOverrides}
            disabled={!hasOverrides(diagram) || saving}
            title="Remove all manual positions"
          >
            Reset overrides
          </button>
        </div>
      </header>
      <main className="workspace">
        {diagram && !loading ? (
          <DiagramCanvas diagram={diagram} onNodeMove={handleNodeMove} onEdgeMove={handleEdgeMove} />
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
