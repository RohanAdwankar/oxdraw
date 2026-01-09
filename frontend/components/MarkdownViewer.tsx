'use client';

import { useCallback, useMemo, useState } from 'react';
import ReactMarkdown, { Components } from 'react-markdown';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { vscDarkPlus } from 'react-syntax-highlighter/dist/esm/styles/prism';
import type { CodeMapMapping } from '../lib/types';

interface MarkdownViewerProps {
  content: string;
  onNavigate?: (file: string, startLine?: number, endLine?: number) => void;
  codeMapMapping?: CodeMapMapping | null;
}

export default function MarkdownViewer({
  content,
  onNavigate,
  codeMapMapping,
}: MarkdownViewerProps) {
  const [selectedLine, setSelectedLine] = useState<number | null>(null);

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

  const hasMappingForLine = useCallback(
    (line: number) => {
      if (!codeMapMapping) return false;
      return !!codeMapMapping.nodes[`line_${line}`];
    },
    [codeMapMapping]
  );

  const handleLineClick = useCallback(
    (line: number) => {
      if (!onNavigate || !codeMapMapping) return;
      
      const mapping = codeMapMapping.nodes[`line_${line}`];
      if (mapping) {
        setSelectedLine(line);
        onNavigate(mapping.file, mapping.start_line, mapping.end_line);
      }
    },
    [codeMapMapping, onNavigate]
  );

  const components: Components = useMemo(
    () => ({
      code({ node, inline, className, children, ...props }: any) {
        const match = /language-(\w+)/.exec(className || '');
        const line = node?.position?.start.line;
        const hasMapping = line ? hasMappingForLine(line) : false;
        const isSelected = selectedLine === line;

        if (!inline && match) {
          return (
            <div
              onClick={(e) => {
                e.stopPropagation();
                if (line) handleLineClick(line);
              }}
              style={{
                cursor: hasMapping ? 'pointer' : 'default',
                borderLeft: isSelected
                  ? '3px solid #ECC94B'
                  : hasMapping
                  ? '3px solid #4299e1'
                  : '3px solid transparent',
                paddingLeft: hasMapping ? '4px' : '0',
                margin: '1em 0',
              }}
            >
              <SyntaxHighlighter
                style={vscDarkPlus}
                language={match[1]}
                PreTag="div"
                {...props}
              >
                {String(children).replace(/\n$/, '')}
              </SyntaxHighlighter>
            </div>
          );
        } else if (inline) {
            const text = String(children).trim();
            // Check for file extension (basic heuristic)
            if (/\.[a-zA-Z0-9]+$/.test(text)) {
                 return (
                    <code 
                        className={className} 
                        {...props} 
                        onClick={(e) => { e.stopPropagation(); onNavigate && onNavigate(text); }}
                        style={{cursor: 'pointer', color: '#3182ce', textDecoration: 'underline'}}
                    >
                        {children}
                    </code>
                 );
            }
            // Check for symbol match
            if (codeMapMapping) {
                const found = Object.values(codeMapMapping.nodes).find(n => n.symbol === text);
                if (found) {
                     return (
                        <code 
                            className={className} 
                            {...props} 
                            onClick={(e) => { e.stopPropagation(); onNavigate && onNavigate(found.file, found.start_line, found.end_line); }}
                            style={{cursor: 'pointer', color: '#3182ce', textDecoration: 'underline'}}
                        >
                            {children}
                        </code>
                     );
                }
            }
        }
        
        return (
          <code className={className} {...props}>
            {children}
          </code>
        );
      },
      p: ({ node, children }) => {
        const line = node?.position?.start.line;
        const hasMapping = line ? hasMappingForLine(line) : false;
        const isSelected = selectedLine === line;
        return (
          <p
            onClick={(e) => {
              e.stopPropagation();
              if (line) handleLineClick(line);
            }}
            className={hasMapping ? 'has-mapping' : ''}
            style={{
              cursor: hasMapping ? 'pointer' : 'default',
              backgroundColor: isSelected
                ? 'rgba(255, 255, 0, 0.1)'
                : hasMapping
                ? 'rgba(66, 153, 225, 0.05)'
                : 'transparent',
              padding: '2px 8px',
              borderLeft: isSelected
                ? '3px solid #ECC94B'
                : hasMapping
                ? '3px solid #4299e1'
                : '3px solid transparent',
              transition: 'background-color 0.15s ease',
            }}
          >
            {children}
          </p>
        );
      },
      // Headers
      h1: ({ node, children }) => {
        const line = node?.position?.start.line;
        const hasMapping = line ? hasMappingForLine(line) : false;
        return (
          <h1
            onClick={() => line && handleLineClick(line)}
            style={{ cursor: hasMapping ? 'pointer' : 'default' }}
          >
            {children}
          </h1>
        );
      },
      h2: ({ node, children }) => {
        const line = node?.position?.start.line;
        const hasMapping = line ? hasMappingForLine(line) : false;
        return (
          <h2
            onClick={() => line && handleLineClick(line)}
            style={{ cursor: hasMapping ? 'pointer' : 'default' }}
          >
            {children}
          </h2>
        );
      },
      h3: ({ node, children }) => {
        const line = node?.position?.start.line;
        const hasMapping = line ? hasMappingForLine(line) : false;
        return (
          <h3
            onClick={() => line && handleLineClick(line)}
            style={{ cursor: hasMapping ? 'pointer' : 'default' }}
          >
            {children}
          </h3>
        );
      },
    }),
    [hasMappingForLine, handleLineClick, selectedLine, codeMapMapping, onNavigate]
  );

  return (
    <div
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
        .markdown-viewer :global(code):not(pre code) {
          background-color: #f7fafc;
          padding: 2px 6px;
          border-radius: 3px;
          font-family: 'Courier New', monospace;
          font-size: 0.9em;
          color: #e53e3e;
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
      `}</style>
      <ReactMarkdown components={components}>{displayContent}</ReactMarkdown>
    </div>
  );
}
