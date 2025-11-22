import React, { useEffect, useRef } from "react";

interface CodePanelProps {
  filePath: string | null;
  content: string | null;
  startLine?: number;
  endLine?: number;
  onClose: () => void;
  onLineClick?: (line: number) => void;
}

export default function CodePanel({
  filePath,
  content,
  startLine,
  endLine,
  onClose,
  onLineClick,
}: CodePanelProps) {
  const codeRef = useRef<HTMLElement>(null);

  useEffect(() => {
    if (startLine) {
      const element = document.getElementById(`line-${startLine}`);
      if (element) {
        element.scrollIntoView({ behavior: "smooth", block: "center" });
      }
    }
  }, [startLine, content]);

  if (!filePath) {
    return (
      <aside className="code-panel empty">
        <div className="panel-header">
          <span className="panel-title">Codebase</span>
        </div>
        <div className="placeholder">Select a node to view code</div>
      </aside>
    );
  }

  const lines = content ? content.split("\n") : [];

  return (
    <aside className="code-panel">
      <div className="panel-header">
        <span className="panel-title">Codebase</span>
        <span className="panel-path">{filePath}</span>
        <button onClick={onClose} className="close-button">×</button>
      </div>
      <div className="code-content">
        <pre>
          <code>
            {lines.map((line, index) => {
              const lineNumber = index + 1;
              const isHighlighted =
                startLine && endLine && lineNumber >= startLine && lineNumber <= endLine;
              return (
                <div
                  key={index}
                  className={`code-line ${isHighlighted ? "highlighted" : ""}`}
                  id={`line-${lineNumber}`}
                  onClick={() => onLineClick?.(lineNumber)}
                  style={{ cursor: onLineClick ? "pointer" : "default" }}
                >
                  <span className="line-number">{lineNumber}</span>
                  <span className="line-text">{line}</span>
                </div>
              );
            })}
          </code>
        </pre>
      </div>
    </aside>
  );
}
