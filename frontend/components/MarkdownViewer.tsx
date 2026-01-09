'use client';

import React, { useCallback, useEffect, useMemo, useState } from 'react';
import ReactMarkdown, { Components } from 'react-markdown';
import { Prism as SyntaxHighlighter } from 'react-syntax-highlighter';
import { oneDark, oneLight } from 'react-syntax-highlighter/dist/esm/styles/prism';
import type { CodeLocation, CodeMapMapping } from '../lib/types';

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
  const [isDark, setIsDark] = useState(false);

  useEffect(() => {
    const observer = new MutationObserver((mutations) => {
      mutations.forEach((mutation) => {
        if (mutation.type === 'attributes' && mutation.attributeName === 'data-theme') {
          setIsDark(document.body.getAttribute('data-theme') === 'dark');
        }
      });
    });

    observer.observe(document.body, { attributes: true });
    setIsDark(document.body.getAttribute('data-theme') === 'dark');
    return () => observer.disconnect();
  }, []);

  const symbolIndex = useMemo(() => {
    if (!codeMapMapping) {
      return null;
    }

    const index = new Map<string, CodeLocation>();
    for (const location of Object.values(codeMapMapping.nodes)) {
      if (location.symbol && !index.has(location.symbol)) {
        index.set(location.symbol, location);
      }
    }
    return index;
  }, [codeMapMapping]);

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
      pre: ({ node, children, ...props }: any) => {
        const line = node?.position?.start?.line;
        const hasMapping = line ? hasMappingForLine(line) : false;
        const isSelected = line ? selectedLine === line : false;

        const firstChild = Array.isArray(children) ? children[0] : children;
        const codeElement = React.isValidElement(firstChild) ? (firstChild as any) : null;
        const codeClassName = codeElement?.props?.className as string | undefined;
        const match = /language-(\w+)/.exec(codeClassName ?? '');
        const rawCode = codeElement?.props?.children;
        const codeText = typeof rawCode === 'string' ? rawCode : Array.isArray(rawCode) ? rawCode.join('') : String(rawCode ?? '');
        const normalizedCode = codeText.replace(/\n$/, '');

        return (
          <div
            onClick={(e) => {
              e.stopPropagation();
              if (line) {
                handleLineClick(line);
              }
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
              background: 'transparent',
            }}
          >
            {match ? (
              <SyntaxHighlighter
                style={isDark ? oneDark : oneLight}
                language={match[1]}
                PreTag="div"
                {...props}
              >
                {normalizedCode}
              </SyntaxHighlighter>
            ) : (
              <pre {...props}>
                <code className={codeClassName}>{normalizedCode}</code>
              </pre>
            )}
          </div>
        );
      },
      code({ className, children, ...props }: any) {
        const text = String(children).trim();
        const hasFileExtension = /\.[a-zA-Z0-9]+$/.test(text);
        const looksLikePath = text.includes('/') || hasFileExtension;
        const looksLikeIdentifier = /^[a-zA-Z_][a-zA-Z0-9_]*$/.test(text);

        const mapped = symbolIndex?.get(text);
        const shouldLink = Boolean(mapped) || looksLikePath || looksLikeIdentifier;

        if (shouldLink) {
          return (
            <code
              className={className}
              {...props}
              onClick={(e) => {
                e.stopPropagation();
                if (!onNavigate) {
                  return;
                }

                if (mapped) {
                  // Delegate to the parent handler so it can also populate the occurrences UI.
                  onNavigate(text);
                  return;
                }

                onNavigate(text);
              }}
              style={{ cursor: 'pointer', color: '#3182ce', textDecoration: 'underline' }}
            >
              {children}
            </code>
          );
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
        background: 'var(--bg-primary)',
        color: 'var(--text-primary)',
      }}
    >
      <style jsx>{`
        .markdown-viewer :global(h1) {
          font-size: 2rem;
          font-weight: bold;
          margin: 1.5rem 0 1rem 0;
          color: var(--text-primary);
        }
        .markdown-viewer :global(h2) {
          font-size: 1.5rem;
          font-weight: bold;
          margin: 1.25rem 0 0.75rem 0;
          color: var(--text-primary);
        }
        .markdown-viewer :global(h3) {
          font-size: 1.25rem;
          font-weight: bold;
          margin: 1rem 0 0.5rem 0;
          color: var(--text-primary);
        }
        .markdown-viewer :global(p) {
          margin: 0.5rem 0;
          line-height: 1.6;
          color: var(--text-primary);
        }
        .markdown-viewer :global(code):not(pre code) {
          background: var(--bg-tertiary);
          padding: 2px 6px;
          border-radius: 3px;
          font-family: 'Courier New', monospace;
          font-size: 0.9em;
          color: var(--text-primary);
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
          border-left: 4px solid var(--border-color);
          padding-left: 1rem;
          margin: 0.75rem 0;
          color: var(--text-secondary);
          font-style: italic;
        }
        .markdown-viewer :global(a) {
          color: var(--accent-primary);
          text-decoration: underline;
        }
        .markdown-viewer :global(a:hover) {
          color: var(--accent-hover);
        }
        .markdown-viewer :global(hr) {
          border: none;
          border-top: 1px solid var(--border-color);
          margin: 1.5rem 0;
        }
        .markdown-viewer :global(table) {
          border-collapse: collapse;
          width: 100%;
          margin: 0.75rem 0;
        }
        .markdown-viewer :global(th),
        .markdown-viewer :global(td) {
          border: 1px solid var(--border-color);
          padding: 8px 12px;
          text-align: left;
        }
        .markdown-viewer :global(th) {
          background: var(--bg-secondary);
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
