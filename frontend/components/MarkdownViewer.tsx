'use client';

import { useCallback, useEffect, useMemo, useRef, useState } from 'react';
import ReactMarkdown from 'react-markdown';
import type { CodeMapMapping } from '../lib/types';

interface MarkdownViewerProps {
  content: string;
  onSelectLine?: (lineNumber: number) => void;
  codeMapMapping?: CodeMapMapping | null;
}

export default function MarkdownViewer({
  content,
  onSelectLine,
  codeMapMapping,
}: MarkdownViewerProps) {
  const containerRef = useRef<HTMLDivElement>(null);
  const [selectedLine, setSelectedLine] = useState<number | null>(null);

  // Split content into lines for click detection
  const lines = useMemo(() => content.split('\n'), [content]);

  // Remove HTML mapping comments from display
  const displayContent = useMemo(() => {
    let text = content;

    // Remove OXDRAW MAPPING comment
    const mappingStart = text.indexOf('<!-- OXDRAW MAPPING');
    if (mappingStart !== -1) {
      const mappingEnd = text.indexOf('-->', mappingStart);
      if (mappingEnd !== -1) {
        text = text.substring(0, mappingStart) + text.substring(mappingEnd + 3);
      }
    }

    // Remove OXDRAW META comment
    const metaStart = text.indexOf('<!-- OXDRAW META');
    if (metaStart !== -1) {
      const metaEnd = text.indexOf('-->', metaStart);
      if (metaEnd !== -1) {
        text = text.substring(0, metaStart) + text.substring(metaEnd + 3);
      }
    }

    return text.trim();
  }, [content]);

  const handleClick = useCallback(
    (event: React.MouseEvent<HTMLDivElement>) => {
      if (!codeMapMapping || !onSelectLine || !containerRef.current) return;

      // Get the click position relative to the container
      const rect = containerRef.current.getBoundingClientRect();
      const clickY = event.clientY - rect.top;

      // Estimate line height (approximate)
      const lineHeight = 24; // pixels
      const estimatedLine = Math.floor(clickY / lineHeight) + 1;

      // Check if this line has a mapping
      const lineId = `line_${estimatedLine}`;
      if (codeMapMapping.nodes[lineId]) {
        setSelectedLine(estimatedLine);
        onSelectLine(estimatedLine);
      }
    },
    [codeMapMapping, onSelectLine]
  );

  // Highlight lines with mappings
  const renderMarkdown = useCallback(() => {
    if (!codeMapMapping) {
      return <ReactMarkdown>{displayContent}</ReactMarkdown>;
    }

    // Split into lines and wrap each in a div for click detection
    const markdownLines = displayContent.split('\n');
    const lineMapping: { [key: number]: boolean } = {};

    Object.keys(codeMapMapping.nodes).forEach((key) => {
      const match = key.match(/^line_(\d+)$/);
      if (match) {
        lineMapping[parseInt(match[1], 10)] = true;
      }
    });

    return (
      <div className="markdown-lines">
        {markdownLines.map((line, index) => {
          const lineNumber = index + 1;
          const hasMapping = lineMapping[lineNumber];
          const isSelected = selectedLine === lineNumber;

          return (
            <div
              key={index}
              className={`markdown-line ${hasMapping ? 'has-mapping' : ''} ${
                isSelected ? 'selected' : ''
              }`}
              onClick={() => {
                if (hasMapping && onSelectLine) {
                  setSelectedLine(lineNumber);
                  onSelectLine(lineNumber);
                }
              }}
              style={{
                cursor: hasMapping ? 'pointer' : 'default',
                backgroundColor: isSelected
                  ? 'rgba(255, 255, 0, 0.2)'
                  : hasMapping
                  ? 'rgba(66, 153, 225, 0.05)'
                  : 'transparent',
                padding: '2px 8px',
                borderLeft: hasMapping ? '3px solid #4299e1' : '3px solid transparent',
                transition: 'background-color 0.15s ease',
              }}
            >
              <ReactMarkdown>{line}</ReactMarkdown>
            </div>
          );
        })}
      </div>
    );
  }, [displayContent, codeMapMapping, selectedLine, onSelectLine]);

  return (
    <div
      ref={containerRef}
      className="markdown-viewer"
      style={{
        width: '100%',
        height: '100%',
        overflow: 'auto',
        padding: '24px',
        backgroundColor: '#ffffff',
      }}
    >
      <style jsx>{`
        .markdown-viewer :global(h1) {
          font-size: 2rem;
          font-weight: bold;
          margin: 1.5rem 0 1rem 0;
          color: #1a202c;
        }
        .markdown-viewer :global(h2) {
          font-size: 1.5rem;
          font-weight: bold;
          margin: 1.25rem 0 0.75rem 0;
          color: #2d3748;
        }
        .markdown-viewer :global(h3) {
          font-size: 1.25rem;
          font-weight: bold;
          margin: 1rem 0 0.5rem 0;
          color: #2d3748;
        }
        .markdown-viewer :global(p) {
          margin: 0.5rem 0;
          line-height: 1.6;
          color: #2d3748;
        }
        .markdown-viewer :global(code) {
          background-color: #f7fafc;
          padding: 2px 6px;
          border-radius: 3px;
          font-family: 'Courier New', monospace;
          font-size: 0.9em;
          color: #e53e3e;
        }
        .markdown-viewer :global(pre) {
          background-color: #f7fafc;
          padding: 12px;
          border-radius: 6px;
          overflow-x: auto;
          margin: 0.75rem 0;
        }
        .markdown-viewer :global(pre code) {
          background-color: transparent;
          padding: 0;
          color: #2d3748;
        }
        .markdown-viewer :global(ul),
        .markdown-viewer :global(ol) {
          margin: 0.5rem 0;
          padding-left: 2rem;
        }
        .markdown-viewer :global(li) {
          margin: 0.25rem 0;
          line-height: 1.6;
        }
        .markdown-viewer :global(blockquote) {
          border-left: 4px solid #e2e8f0;
          padding-left: 1rem;
          margin: 0.75rem 0;
          color: #4a5568;
          font-style: italic;
        }
        .markdown-viewer :global(a) {
          color: #3182ce;
          text-decoration: underline;
        }
        .markdown-viewer :global(a:hover) {
          color: #2c5aa0;
        }
        .markdown-viewer :global(hr) {
          border: none;
          border-top: 1px solid #e2e8f0;
          margin: 1.5rem 0;
        }
        .markdown-viewer :global(table) {
          border-collapse: collapse;
          width: 100%;
          margin: 0.75rem 0;
        }
        .markdown-viewer :global(th),
        .markdown-viewer :global(td) {
          border: 1px solid #e2e8f0;
          padding: 8px 12px;
          text-align: left;
        }
        .markdown-viewer :global(th) {
          background-color: #f7fafc;
          font-weight: bold;
        }
        .markdown-line.has-mapping:hover {
          background-color: rgba(66, 153, 225, 0.1) !important;
        }
        .markdown-lines {
          font-size: 15px;
          font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', 'Roboto', 'Oxygen',
            'Ubuntu', 'Cantarell', 'Fira Sans', 'Droid Sans', 'Helvetica Neue', sans-serif;
        }
      `}</style>
      {renderMarkdown()}
    </div>
  );
}
