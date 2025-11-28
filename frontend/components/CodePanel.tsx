import React, { useEffect, useRef, useState, useCallback, useMemo } from "react";
import { Prism as SyntaxHighlighter } from "react-syntax-highlighter";
import { oneDark, oneLight } from "react-syntax-highlighter/dist/esm/styles/prism";

const LINE_HEIGHT_PX = 22;
const OVERSCAN_ROWS = 60;

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

// Custom renderer for a single line to avoid heavy parsing of the whole file
const CodeLine = React.memo(({ 
  line, 
  lineNumber, 
  language, 
  isDark, 
  isHighlighted, 
  isWrapped,
  onLineClick 
}: { 
  line: string; 
  lineNumber: number; 
  language: string; 
  isDark: boolean; 
  isHighlighted: boolean; 
  isWrapped: boolean;
  onLineClick?: (line: number) => void;
}) => {
  return (
    <div 
      className={`code-line-row ${isHighlighted ? "highlighted" : ""}`}
      onClick={() => onLineClick?.(lineNumber)}
      style={{
        display: "flex",
        width: "100%",
        backgroundColor: isHighlighted ? (isDark ? "rgba(253, 230, 138, 0.15)" : "rgba(253, 230, 138, 0.5)") : undefined,
        cursor: onLineClick ? "pointer" : "default",
      }}
    >
      <div className="line-number" style={{ 
        minWidth: "2.5rem", 
        paddingRight: "0.5rem", 
        textAlign: "right", 
        color: "var(--text-secondary)", 
        userSelect: "none",
        flexShrink: 0
      }}>
        {lineNumber}
      </div>
      <div className="line-content" style={{ 
        flex: 1, 
        whiteSpace: isWrapped ? "pre-wrap" : "pre",
        wordBreak: isWrapped ? "break-all" : "normal",
        fontFamily: '"JetBrains Mono", monospace',
        fontSize: "0.85rem",
        lineHeight: "1.5"
      }}>
        <SyntaxHighlighter
          language={language}
          style={isDark ? oneDark : oneLight}
          PreTag="span"
          CodeTag="span"
          showLineNumbers={false}
          customStyle={{
            margin: 0,
            padding: 0,
            background: "transparent",
          }}
        >
          {line || " "}
        </SyntaxHighlighter>
      </div>
    </div>
  );
}, (prev, next) => {
  return (
    prev.line === next.line &&
    prev.lineNumber === next.lineNumber &&
    prev.language === next.language &&
    prev.isDark === next.isDark &&
    prev.isHighlighted === next.isHighlighted &&
    prev.isWrapped === next.isWrapped
  );
});

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
  const viewportRef = useRef<HTMLDivElement>(null);
  const [viewportHeight, setViewportHeight] = useState(0);
  const [scrollTop, setScrollTop] = useState(0);

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

  const lines = useMemo(() => content ? content.split("\n") : [], [content]);
  const language = useMemo(() => filePath ? getLanguage(filePath) : "text", [filePath]);

  useEffect(() => {
    if (startLine && !isCollapsed && viewportRef.current) {
      const targetOffset = (startLine - 1) * LINE_HEIGHT_PX;
      const viewport = viewportRef.current;
      const nextScroll = Math.max(targetOffset - viewport.clientHeight / 2, 0);
      viewport.scrollTo({ top: nextScroll, behavior: "smooth" });
    }
  }, [startLine, isCollapsed, lines.length]);

  useEffect(() => {
    if (!viewportRef.current) return;
    const element = viewportRef.current;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        if (entry.contentRect) {
          setViewportHeight(entry.contentRect.height);
        }
      }
    });
    observer.observe(element);
    setViewportHeight(element.clientHeight);
    return () => observer.disconnect();
  }, []);

  const handleScroll = useCallback(() => {
    if (!viewportRef.current) return;
    setScrollTop(viewportRef.current.scrollTop);
  }, []);

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

  const totalHeight = lines.length * LINE_HEIGHT_PX;
  const startIndex = Math.max(0, Math.floor(scrollTop / LINE_HEIGHT_PX) - OVERSCAN_ROWS);
  const visibleCount = Math.ceil((viewportHeight || 0) / LINE_HEIGHT_PX) + OVERSCAN_ROWS * 2;
  const endIndex = Math.min(lines.length, startIndex + visibleCount);
  const offsetY = startIndex * LINE_HEIGHT_PX;
  const visibleLines = lines.slice(startIndex, endIndex);

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
      <div
        ref={viewportRef}
        className="code-content"
        style={{ flex: 1, overflow: "auto", padding: 0, position: "relative" }}
        onScroll={handleScroll}
      >
        <div style={{ height: totalHeight, position: "relative", width: "100%" }}>
          <div style={{ position: "absolute", top: 0, left: 0, width: "100%", transform: `translateY(${offsetY}px)` }}>
            {visibleLines.map((line, idx) => {
              const lineNumber = startIndex + idx + 1;
              const isHighlighted = startLine && endLine ? (lineNumber >= startLine && lineNumber <= endLine) : false;
              return (
                <CodeLine
                  key={lineNumber}
                  line={line}
                  lineNumber={lineNumber}
                  language={language}
                  isDark={isDark}
                  isHighlighted={!!isHighlighted}
                  isWrapped={isWrapped}
                  onLineClick={onLineClick}
                />
              );
            })}
          </div>
        </div>
      </div>
    </aside>
  );
}
