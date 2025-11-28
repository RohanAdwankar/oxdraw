import React, { useEffect, useRef, useState, useCallback } from "react";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark, oneLight } from "react-syntax-highlighter/dist/esm/styles/prism";

interface CodePanelProps {
  filePath: string | null;
  content: string | null;
  startLine?: number;
  endLine?: number;
  onClose: () => void;
  onLineClick?: (line: number) => void;
}

const getLanguage = (filename: string) => {
  if (filename.endsWith(".rs")) return "rust";
  if (filename.endsWith(".tsx") || filename.endsWith(".ts")) return "typescript";
  if (filename.endsWith(".js")) return "javascript";
  if (filename.endsWith(".css")) return "css";
  if (filename.endsWith(".html")) return "html";
  if (filename.endsWith(".json")) return "json";
  return "text";
};

export default function CodePanel({
  filePath,
  content,
  startLine,
  endLine,
  onClose,
  onLineClick,
}: CodePanelProps) {
  const [width, setWidth] = useState(500);
  const [isResizing, setIsResizing] = useState(false);
  const [isWrapped, setIsWrapped] = useState(false);
  const [isCollapsed, setIsCollapsed] = useState(false);
  const codeRef = useRef<HTMLElement>(null);

  // Detect theme from body attribute (hacky but works without context)
  const [isDark, setIsDark] = useState(false);
  useEffect(() => {
    const observer = new MutationObserver((mutations) => {
      mutations.forEach((mutation) => {
        if (mutation.type === "attributes" && mutation.attributeName === "data-theme") {
          setIsDark(document.body.getAttribute("data-theme") === "dark");
        }
      });
    });
    observer.observe(document.body, { attributes: true });
    setIsDark(document.body.getAttribute("data-theme") === "dark");
    return () => observer.disconnect();
  }, []);

  useEffect(() => {
    if (startLine && !isCollapsed) {
      // Small delay to allow syntax highlighter to render
      setTimeout(() => {
        const element = document.getElementById(`line-${startLine}`);
        if (element) {
          element.scrollIntoView({ behavior: "smooth", block: "center" });
        }
      }, 100);
    }
  }, [startLine, content, isCollapsed]);

  const startResizing = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsResizing(true);
  }, []);

  useEffect(() => {
    if (!isResizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      const newWidth = document.body.clientWidth - e.clientX;
      setWidth(Math.max(300, Math.min(newWidth, 1200)));
    };

    const handleMouseUp = () => {
      setIsResizing(false);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);

    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isResizing]);

  if (isCollapsed) {
    return (
      <div className="collapse-button collapsed-right" onClick={() => setIsCollapsed(false)} title="Expand Code Panel">
        ‹
      </div>
    );
  }

  if (!filePath) {
    return (
      <aside className="code-panel empty" style={{ width }}>
        <div
          className="resize-handle"
          onMouseDown={startResizing}
          style={{
            position: "absolute",
            left: 0,
            top: 0,
            bottom: 0,
            width: "8px",
            cursor: "col-resize",
            zIndex: 10,
            background: "transparent",
          }}
        />
        <button className="collapse-button right" onClick={() => setIsCollapsed(true)} title="Collapse Code Panel">
          ›
        </button>
        <div className="panel-header">
          <span className="panel-title">Codebase</span>
        </div>
        <div className="placeholder">Select a node to view code</div>
      </aside>
    );
  }

  const language = getLanguage(filePath);

  return (
    <aside className="code-panel" style={{ width }}>
      <div
        className="resize-handle"
        onMouseDown={startResizing}
        style={{
          position: "absolute",
          left: 0,
          top: 0,
          bottom: 0,
          width: "8px",
          cursor: "col-resize",
          zIndex: 10,
          background: "transparent",
        }}
      />
      <button className="collapse-button right" onClick={() => setIsCollapsed(true)} title="Collapse Code Panel">
        ›
      </button>
      <div className="panel-header">
        <span className="panel-title">Codebase</span>
        <span className="panel-path">{filePath}</span>
        <div className="panel-actions">
          <button
            className={isWrapped ? "active" : ""}
            onClick={() => setIsWrapped(!isWrapped)}
            title="Toggle Word Wrap"
          >
            Wrap
          </button>
          <button onClick={onClose} className="close-button">×</button>
        </div>
      </div>
      <div className="code-content" style={{ padding: 0 }}>
        <SyntaxHighlighter
          language={language}
          style={isDark ? oneDark : oneLight}
          showLineNumbers={true}
          wrapLines={true}
          wrapLongLines={isWrapped}
          lineProps={(lineNumber) => {
            const isHighlighted = startLine && endLine && lineNumber >= startLine && lineNumber <= endLine;
            return {
              style: {
                display: "block",
                backgroundColor: isHighlighted ? (isDark ? "rgba(253, 230, 138, 0.15)" : "rgba(253, 230, 138, 0.5)") : undefined,
                cursor: onLineClick ? "pointer" : "default",
              },
              onClick: () => onLineClick?.(lineNumber),
              id: `line-${lineNumber}`
            };
          }}
          customStyle={{
            margin: 0,
            padding: "1rem",
            fontSize: "0.85rem",
            lineHeight: "1.5",
            background: "transparent",
          }}
        >
          {content || ""}
        </SyntaxHighlighter>
      </div>
    </aside>
  );
}
