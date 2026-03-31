'use client';

import {
  ChangeEvent,
  KeyboardEvent as ReactKeyboardEvent,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import WasmDiagramCanvas from "../components/WasmDiagramCanvas";
import MarkdownViewer from "../components/MarkdownViewer";
import CodePanel from "../components/CodePanel";
import type { CanvasContextMenuRequest, CanvasToolMode } from "../components/diagramCanvasTypes";
import {
  buildLocalShareUrl,
  deleteEdge,
  deleteNode,
  fetchDiagram,
  isLocalMode,
  updateLayout,
  updateNodeImage,
  updateSource,
  updateStyle,
  fetchCodeMapMapping,
  fetchCodeMapFile,
  openInEditor,
  searchCodebase,
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
  CodeMapMapping,
  CodeLocation,
  SearchResult,
  NodeShape,
} from "../lib/types";
import {
  addEdgeToSource,
  addNodeToSource,
  makeUniqueNodeId,
  updateNodeShapeInSource,
} from "../lib/sourceBuilder";

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
  "double-circle": "#BFDBFE",
  diamond: "#FBCFE8",
  subroutine: "#FED7AA",
  cylinder: "#BBF7D0",
  hexagon: "#FCA5A5",
  parallelogram: "#C7D2FE",
  "parallelogram-alt": "#A5F3FC",
  trapezoid: "#FCE7F3",
  "trapezoid-alt": "#FCD5CE",
  asymmetric: "#F5D0FE",
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

const NODE_SHAPE_OPTIONS: Array<{ value: NodeShape; label: string }> = [
  { value: "rectangle", label: "Rectangle" },
  { value: "stadium", label: "Stadium" },
  { value: "circle", label: "Circle" },
  { value: "double-circle", label: "Double circle" },
  { value: "diamond", label: "Diamond" },
  { value: "subroutine", label: "Subroutine" },
  { value: "cylinder", label: "Cylinder" },
  { value: "hexagon", label: "Hexagon" },
  { value: "parallelogram", label: "Parallelogram" },
  { value: "parallelogram-alt", label: "Parallelogram alt" },
  { value: "trapezoid", label: "Trapezoid" },
  { value: "trapezoid-alt", label: "Trapezoid alt" },
  { value: "asymmetric", label: "Asymmetric" },
];

type ContextMenuState = CanvasContextMenuRequest | null;

const HEX_COLOR_RE = /^#([0-9a-f]{6})$/i;

const PADDING_PRECISION = 1000;
const PADDING_EPSILON = 0.001;
const MAX_IMAGE_FILE_BYTES = 10 * 1024 * 1024;

const formatByteSize = (bytes: number): string => {
  if (!Number.isFinite(bytes) || bytes <= 0) {
    return "0 B";
  }
  const units = ["B", "KB", "MB", "GB"];
  let value = bytes;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  const decimals = unitIndex === 0 ? 0 : value < 10 ? 1 : 0;
  return `${value.toFixed(decimals)} ${units[unitIndex]}`;
};

const blobToBase64 = (blob: Blob): Promise<string> =>
  new Promise((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => {
      const result = reader.result;
      if (typeof result !== "string") {
        reject(new Error("Failed to encode image."));
        return;
      }
      const commaIndex = result.indexOf(",");
      if (commaIndex === -1) {
        reject(new Error("Failed to encode image."));
        return;
      }
      resolve(result.slice(commaIndex + 1));
    };
    reader.onerror = () => {
      reject(reader.error ?? new Error("Failed to encode image."));
    };
    reader.readAsDataURL(blob);
  });

const loadImageFromBlob = (blob: Blob): Promise<HTMLImageElement> =>
  new Promise((resolve, reject) => {
    const url = URL.createObjectURL(blob);
    const image = new Image();
    image.onload = () => {
      URL.revokeObjectURL(url);
      resolve(image);
    };
    image.onerror = () => {
      URL.revokeObjectURL(url);
      reject(new Error("Unable to read image file."));
    };
    image.src = url;
  });

const resizeImageToLimit = async (
  image: HTMLImageElement,
  sourceBlob: Blob,
  maxBytes: number
): Promise<{ blob: Blob; resized: boolean; fits: boolean }> => {
  const canvas = document.createElement("canvas");
  const context = canvas.getContext("2d");
  if (!context) {
    throw new Error("Canvas support is required to resize images.");
  }

  if (sourceBlob.size <= maxBytes) {
    return { blob: sourceBlob, resized: false, fits: true };
  }

  const MIN_SCALE = 0.05;
  const STEP = 0.75;

  let currentScale = Math.sqrt(maxBytes / sourceBlob.size);
  if (!Number.isFinite(currentScale) || currentScale >= 0.99) {
    currentScale = 0.95;
  }
  currentScale = Math.min(currentScale, 0.95);
  currentScale = Math.max(currentScale, MIN_SCALE);

  let blob: Blob | null = null;
  let fits = false;
  let attempts = 0;

  while (attempts < 10 && currentScale >= MIN_SCALE) {
    const targetWidth = Math.max(1, Math.round(image.width * currentScale));
    const targetHeight = Math.max(1, Math.round(image.height * currentScale));

    canvas.width = targetWidth;
    canvas.height = targetHeight;

    context.clearRect(0, 0, targetWidth, targetHeight);
    context.drawImage(image, 0, 0, targetWidth, targetHeight);

    blob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, "image/png")
    );

    if (!blob) {
      throw new Error("Failed to encode resized image.");
    }

    if (blob.size <= maxBytes) {
      fits = true;
      break;
    }

    currentScale *= STEP;
    attempts += 1;
  }

  if (!blob) {
    throw new Error("Failed to process image.");
  }

  return { blob, resized: true, fits };
};

const ensureImageWithinLimit = async (
  file: File,
  maxBytes: number
): Promise<{
  base64: string;
  resized: boolean;
  originalSize: number;
  finalSize: number;
}> => {
  if (file.size <= maxBytes) {
    const base64 = await blobToBase64(file);
    return {
      base64,
      resized: false,
      originalSize: file.size,
      finalSize: file.size,
    };
  }

  const image = await loadImageFromBlob(file);
  const width = image.naturalWidth || image.width;
  const height = image.naturalHeight || image.height;
  if (!width || !height) {
    throw new Error("Unable to read image dimensions.");
  }

  const { blob, fits } = await resizeImageToLimit(image, file, maxBytes);

  if (!fits || blob.size > maxBytes) {
    throw new Error(
      `Image is too large to upload. Please use an image smaller than ${formatByteSize(
        maxBytes
      )}.`
    );
  }

  const base64 = await blobToBase64(blob);
  return {
    base64,
    resized: true,
    originalSize: file.size,
    finalSize: blob.size,
  };
};

const formatPaddingValue = (value: number): string => {
  if (!Number.isFinite(value)) {
    return "0";
  }
  const rounded = Math.round(value * PADDING_PRECISION) / PADDING_PRECISION;
  const fixed = rounded.toFixed(3);
  const trimmed = fixed.replace(/(\.\d*?)0+$/, "$1").replace(/\.$/, "");
  return trimmed;
};

const normalizePadding = (value: number): number => {
  if (!Number.isFinite(value) || Number.isNaN(value) || value < 0) {
    return 0;
  }
  const clamped = Math.max(0, value);
  return Math.round(clamped * PADDING_PRECISION) / PADDING_PRECISION;
};

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

const hasCodeAnnotations = (source: string): boolean =>
  /^\s*%%\s*OXDRAW CODE\b/m.test(source);

const loadImageFromUrl = (url: string): Promise<HTMLImageElement> =>
  new Promise((resolve, reject) => {
    const image = new Image();
    image.onload = () => resolve(image);
    image.onerror = () => reject(new Error("Unable to render diagram for PNG download."));
    image.src = url;
  });

const svgMarkupToPngBlob = async (svgMarkup: string): Promise<Blob> => {
  const svgBlob = new Blob([svgMarkup], { type: "image/svg+xml;charset=utf-8" });
  const svgUrl = URL.createObjectURL(svgBlob);
  try {
    const image = await loadImageFromUrl(svgUrl);
    const width = image.naturalWidth || image.width;
    const height = image.naturalHeight || image.height;
    if (!width || !height) {
      throw new Error("Diagram has no renderable size.");
    }

    const canvas = document.createElement("canvas");
    canvas.width = width;
    canvas.height = height;

    const context = canvas.getContext("2d");
    if (!context) {
      throw new Error("Canvas support is required to download PNG files.");
    }

    context.clearRect(0, 0, width, height);
    context.drawImage(image, 0, 0, width, height);

    const pngBlob = await new Promise<Blob | null>((resolve) =>
      canvas.toBlob(resolve, "image/png")
    );
    if (!pngBlob) {
      throw new Error("Failed to encode PNG download.");
    }
    return pngBlob;
  } finally {
    URL.revokeObjectURL(svgUrl);
  }
};

const downloadBlob = (blob: Blob, filename: string): void => {
  const url = URL.createObjectURL(blob);
  const link = document.createElement("a");
  link.href = url;
  link.download = filename;
  document.body.appendChild(link);
  link.click();
  link.remove();
  URL.revokeObjectURL(url);
};

const toPngFilename = (sourcePath: string): string => {
  const basename = sourcePath.split("/").pop() ?? "diagram";
  return basename.replace(/\.[^.]+$/, "") + ".png";
};

const copyTextToClipboard = async (text: string): Promise<void> => {
  if (navigator.clipboard?.writeText) {
    await navigator.clipboard.writeText(text);
    return;
  }

  const textarea = document.createElement("textarea");
  textarea.value = text;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "absolute";
  textarea.style.left = "-9999px";
  document.body.appendChild(textarea);
  textarea.select();
  try {
    const copied = document.execCommand("copy");
    if (!copied) {
      throw new Error("Clipboard copy failed.");
    }
  } finally {
    textarea.remove();
  }
};

const LOCAL_MODE = isLocalMode();

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
  const [selectedNodeIds, setSelectedNodeIds] = useState<string[]>([]);
  const [selectedEdgeId, setSelectedEdgeId] = useState<string | null>(null);
  const [imagePaddingValue, setImagePaddingValue] = useState<string>("");
  const [dragging, setDragging] = useState(false);
  const [codeMapMapping, setCodeMapMapping] = useState<CodeMapMapping | null>(null);
  const [codeMapMode, setCodeMapMode] = useState(false);
  const [isCodeAnnotated, setIsCodeAnnotated] = useState(false);
  const [codedownMode, setCodedownMode] = useState(false);
  const [markdownContent, setMarkdownContent] = useState<string>("");
  const [selectedFile, setSelectedFile] = useState<{ path: string; content: string } | null>(null);
  const [highlightedLines, setHighlightedLines] = useState<{ start: number; end: number } | null>(null);
  const [theme, setTheme] = useState<"light" | "dark">("light");
  const [searchResults, setSearchResults] = useState<SearchResult[] | null>(null);
  const [activeSearchIndex, setActiveSearchIndex] = useState<number>(0);
  const [leftPanelWidth, setLeftPanelWidth] = useState(280);
  const [isLeftPanelResizing, setIsLeftPanelResizing] = useState(false);
  const [isLeftPanelCollapsed, setIsLeftPanelCollapsed] = useState(false);

  const [rightPanelWidth, setRightPanelWidth] = useState(380);
  const [isRightPanelResizing, setIsRightPanelResizing] = useState(false);
  const [isRightPanelCollapsed, setIsRightPanelCollapsed] = useState(false);
  const [svgMarkup, setSvgMarkup] = useState("");
  const [downloadingPng, setDownloadingPng] = useState(false);
  const [sharingLink, setSharingLink] = useState(false);
  const [shareCopied, setShareCopied] = useState(false);
  const [activeTool, setActiveTool] = useState<CanvasToolMode>("select");
  const [toolNodeShape, setToolNodeShape] = useState<NodeShape>("rectangle");
  const [toolEdgeKind, setToolEdgeKind] = useState<EdgeKind>("solid");
  const [toolEdgeDirected, setToolEdgeDirected] = useState(true);
  const [connectStartNodeId, setConnectStartNodeId] = useState<string | null>(null);
  const [addNodeSourceId, setAddNodeSourceId] = useState<string | null>(null);
  const [contextMenu, setContextMenu] = useState<ContextMenuState>(null);
  const [pendingPlacedNode, setPendingPlacedNode] = useState<{ id: string; point: Point } | null>(null);
  const [viewportControls, setViewportControls] = useState<{
    zoomIn: () => void;
    zoomOut: () => void;
    resetZoom: () => void;
    getScale: () => number;
  } | null>(null);

  const saveTimer = useRef<number | null>(null);
  const lastSubmittedSource = useRef<string | null>(null);
  const nodeImageInputRef = useRef<HTMLInputElement | null>(null);
  const imagePaddingValueRef = useRef(imagePaddingValue);
  const shareCopiedTimer = useRef<number | null>(null);
  const reservedNodeIdsRef = useRef<Set<string>>(new Set());

  useEffect(() => {
    return () => {
      if (shareCopiedTimer.current !== null) {
        window.clearTimeout(shareCopiedTimer.current);
      }
    };
  }, []);

  useEffect(() => {
    document.body.setAttribute("data-theme", theme);
  }, [theme]);

  useEffect(() => {
    const handleWindowPointerDown = () => setContextMenu(null);
    window.addEventListener("pointerdown", handleWindowPointerDown);
    return () => window.removeEventListener("pointerdown", handleWindowPointerDown);
  }, []);

  useEffect(() => {
    if (activeTool !== "connect" && connectStartNodeId) {
      setConnectStartNodeId(null);
    }
  }, [activeTool, connectStartNodeId]);

  useEffect(() => {
    if (activeTool !== "add-node" && addNodeSourceId) {
      setAddNodeSourceId(null);
    }
  }, [activeTool, addNodeSourceId]);

  useEffect(() => {
    if (!diagram || !connectStartNodeId) {
      return;
    }
    if (!diagram.nodes.some((node) => node.id === connectStartNodeId)) {
      setConnectStartNodeId(null);
    }
  }, [connectStartNodeId, diagram]);

  useEffect(() => {
    if (!diagram) {
      return;
    }
    const existingIds = new Set(diagram.nodes.map((node) => node.id));
    reservedNodeIdsRef.current.forEach((id) => {
      if (existingIds.has(id)) {
        reservedNodeIdsRef.current.delete(id);
      }
    });
  }, [diagram]);

  const toggleTheme = useCallback(() => {
    setTheme((prev) => (prev === "light" ? "dark" : "light"));
  }, []);

  const startLeftPanelResizing = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsLeftPanelResizing(true);
  }, []);

  const startRightPanelResizing = useCallback((e: React.MouseEvent) => {
    e.preventDefault();
    setIsRightPanelResizing(true);
  }, []);

  useEffect(() => {
    if (!isLeftPanelResizing && !isRightPanelResizing) return;

    const handleMouseMove = (e: MouseEvent) => {
      if (isLeftPanelResizing) {
        setLeftPanelWidth(Math.max(200, Math.min(e.clientX, 600)));
      } else if (isRightPanelResizing) {
        const newWidth = document.body.clientWidth - e.clientX;
        setRightPanelWidth(Math.max(300, newWidth));
      }
    };

    const handleMouseUp = () => {
      setIsLeftPanelResizing(false);
      setIsRightPanelResizing(false);
    };

    window.addEventListener("mousemove", handleMouseMove);
    window.addEventListener("mouseup", handleMouseUp);

    return () => {
      window.removeEventListener("mousemove", handleMouseMove);
      window.removeEventListener("mouseup", handleMouseUp);
    };
  }, [isLeftPanelResizing, isRightPanelResizing]);

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

useEffect(() => {
  if (selectedNode?.image) {
    setImagePaddingValue(formatPaddingValue(selectedNode.image.padding));
  } else {
    setImagePaddingValue("");
  }
}, [selectedNode?.id, selectedNode?.image?.padding]);

useEffect(() => {
  imagePaddingValueRef.current = imagePaddingValue;
}, [imagePaddingValue]);

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
      const annotated = hasCodeAnnotations(data.source);
      setIsCodeAnnotated(annotated);
      if (!annotated) {
        setCodeMapMode(false);
        setCodeMapMapping(null);
      }

      if (data.sourcePath.endsWith('.md')) {
        setCodedownMode(true);
        setMarkdownContent(data.source);
      } else {
        setCodedownMode(false);
      }

      setSelectedNodeId((current) =>
        current && data.nodes.some((node) => node.id === current) ? current : null
      );
      setSelectedNodeIds((current) =>
        current.filter((id) => data.nodes.some((node) => node.id === id))
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
  void loadDiagram()
    .then((data) => {
      if (!hasCodeAnnotations(data.source)) {
        setCodeMapMapping(null);
        setCodeMapMode(false);
        return;
      }
      fetchCodeMapMapping()
        .then((mapping) => {
          setCodeMapMapping(mapping);
        })
        .catch(() => {
          setCodeMapMapping(null);
          setCodeMapMode(false);
        });
    })
    .catch(() => undefined);
}, [loadDiagram]);

useEffect(() => {
  if (codeMapMode && selectedNodeId && codeMapMapping) {
    const location = codeMapMapping.nodes[selectedNodeId];
    if (location) {
      fetchCodeMapFile(location.file).then((content) => {
        setSelectedFile({ path: location.file, content });
        if (location.start_line && location.end_line) {
          setHighlightedLines({ start: location.start_line, end: location.end_line });
        } else {
          setHighlightedLines(null);
        }
      }).catch((err) => {
        console.error("Failed to fetch file", err);
        setSelectedFile(null);
      });
    } else {
      setSelectedFile(null);
      setHighlightedLines(null);
    }
  }
}, [codeMapMode, selectedNodeId, codeMapMapping]);

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
    ganttStyle?: {
      rowFillEven?: string | null;
      rowFillOdd?: string | null;
      taskFill?: string | null;
      milestoneFill?: string | null;
      taskText?: string | null;
      milestoneText?: string | null;
    };
  }) => {
    const hasNodeStyles = update.nodeStyles && Object.keys(update.nodeStyles).length > 0;
    const hasEdgeStyles = update.edgeStyles && Object.keys(update.edgeStyles).length > 0;
    const hasGanttStyle = update.ganttStyle && Object.keys(update.ganttStyle).length > 0;
    if (!hasNodeStyles && !hasEdgeStyles && !hasGanttStyle) {
      return;
    }

    try {
      setSaving(true);
      setError(null);
      await updateStyle({
        nodeStyles: update.nodeStyles,
        edgeStyles: update.edgeStyles,
        ganttStyle: update.ganttStyle,
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

const handleGanttStyleChange = useCallback(
  (
    key: "rowFillEven" | "rowFillOdd" | "taskFill" | "milestoneFill" | "taskText" | "milestoneText",
    value: string
  ) => {
    if (!diagram || diagram.kind !== "gantt" || !diagram.gantt) {
      return;
    }
    const normalized = normalizeColorInput(value);
    const currentValue = normalizeColorInput(diagram.gantt.style[key]);
    if (normalized === currentValue) {
      return;
    }
    void submitStyleUpdate({
      ganttStyle: {
        [key]: normalized,
      },
    });
  },
  [diagram, submitStyleUpdate]
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

const handleNodeLabelFillChange = useCallback(
  (value: string) => {
    if (!selectedNode || !selectedNode.image) {
      return;
    }
    const normalized = normalizeColorInput(value);
    const baseFill = resolveColor(selectedNode.fillColor, DEFAULT_NODE_COLORS[selectedNode.shape]);
    const currentLabel = resolveColor(selectedNode.labelFillColor, baseFill);
    if (currentLabel === normalized) {
      return;
    }
    void submitStyleUpdate({
      nodeStyles: {
        [selectedNode.id]: {
          labelFill: normalized,
        },
      },
    });
  },
  [selectedNode, submitStyleUpdate]
);

const handleNodeImageFillChange = useCallback(
  (value: string) => {
    if (!selectedNode || !selectedNode.image) {
      return;
    }
    const normalized = normalizeColorInput(value);
    const baseFill = resolveColor(selectedNode.fillColor, DEFAULT_NODE_COLORS[selectedNode.shape]);
    const currentImage = resolveColor(selectedNode.imageFillColor, baseFill);
    if (currentImage === normalized) {
      return;
    }
    void submitStyleUpdate({
      nodeStyles: {
        [selectedNode.id]: {
          imageFill: normalized,
        },
      },
    });
  },
  [selectedNode, submitStyleUpdate]
);

const handleNodeImageFileChange = useCallback(
  async (event: ChangeEvent<HTMLInputElement>) => {
    if (!selectedNode || saving) {
      event.target.value = "";
      return;
    }

    const file = event.target.files && event.target.files[0] ? event.target.files[0] : null;
    event.target.value = "";

    if (!file) {
      return;
    }

    const declaredType = file.type ? file.type.toLowerCase() : "";
    const effectiveType =
      declaredType || (file.name.toLowerCase().endsWith(".png") ? "image/png" : "");

    if (effectiveType !== "image/png") {
      setError("Only PNG images are supported for nodes.");
      return;
    }

    let preparedImage: {
      base64: string;
      resized: boolean;
      originalSize: number;
      finalSize: number;
    } | null = null;

    try {
      preparedImage = await ensureImageWithinLimit(file, MAX_IMAGE_FILE_BYTES);
    } catch (err) {
      const message = (err as Error).message;
      setError(message);
      window.alert(`${message} Maximum allowed size is ${formatByteSize(MAX_IMAGE_FILE_BYTES)}.`);
      return;
    }

    if (!preparedImage) {
      return;
    }

    if (preparedImage.resized) {
      window.alert(
        `The selected image was ${formatByteSize(preparedImage.originalSize)}. We resized it to ${formatByteSize(preparedImage.finalSize)} to stay under the ${formatByteSize(MAX_IMAGE_FILE_BYTES)} limit.`
      );
    }

    try {
      setSaving(true);
      setError(null);
      const fallbackPadding = selectedNode.image ? selectedNode.image.padding : 0;
      const parsedPadding = Number.parseFloat(imagePaddingValueRef.current);
      const nextPadding = Number.isFinite(parsedPadding)
        ? normalizePadding(Math.max(0, parsedPadding))
        : normalizePadding(fallbackPadding);
      await updateNodeImage(selectedNode.id, {
        mimeType: effectiveType,
        data: preparedImage.base64,
        padding: nextPadding,
      });
      setImagePaddingValue(formatPaddingValue(nextPadding));
      await loadDiagram({ silent: true });
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setSaving(false);
    }
  },
  [selectedNode, saving, loadDiagram]
);

const handleNodeImageRemove = useCallback(async () => {
  if (!selectedNode || saving || !selectedNode.image) {
    return;
  }
  try {
    setSaving(true);
    setError(null);
    await updateNodeImage(selectedNode.id, null);
    await loadDiagram({ silent: true });
  } catch (err) {
    setError((err as Error).message);
  } finally {
    setSaving(false);
  }
}, [selectedNode, saving, loadDiagram]);

const handleNodeImagePaddingChange = useCallback((value: string) => {
  setImagePaddingValue(value);
}, []);

const commitNodeImagePadding = useCallback(async () => {
  if (!selectedNode || !selectedNode.image || saving) {
    return;
  }

  const parsed = Number.parseFloat(imagePaddingValue);
  if (!Number.isFinite(parsed)) {
    setImagePaddingValue(formatPaddingValue(selectedNode.image.padding));
    return;
  }

  const normalized = normalizePadding(Math.max(0, parsed));
  const current = normalizePadding(selectedNode.image.padding);
  if (Math.abs(normalized - current) < PADDING_EPSILON) {
    setImagePaddingValue(formatPaddingValue(current));
    return;
  }

  try {
    setSaving(true);
    setError(null);
    await updateNodeImage(selectedNode.id, { padding: normalized });
    setImagePaddingValue(formatPaddingValue(normalized));
    await loadDiagram({ silent: true });
  } catch (err) {
    setError((err as Error).message);
    setImagePaddingValue(formatPaddingValue(current));
  } finally {
    setSaving(false);
  }
}, [selectedNode, saving, imagePaddingValue, loadDiagram]);

const handleNodeImagePaddingBlur = useCallback(() => {
  if (!selectedNode?.image) {
    setImagePaddingValue("");
    return;
  }
  void commitNodeImagePadding();
}, [commitNodeImagePadding, selectedNode]);

const handleNodeImagePaddingKeyDown = useCallback(
  (event: ReactKeyboardEvent<HTMLInputElement>) => {
    if (event.key === "Enter") {
      event.preventDefault();
      void commitNodeImagePadding();
    } else if (event.key === "Escape") {
      event.preventDefault();
      if (selectedNode?.image) {
        setImagePaddingValue(formatPaddingValue(selectedNode.image.padding));
      } else {
        setImagePaddingValue("");
      }
      event.currentTarget.blur();
    }
  },
  [commitNodeImagePadding, selectedNode]
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

const handleLayoutUpdate = useCallback(
  (update: LayoutUpdate) => {
    const hasNodes = update.nodes && Object.keys(update.nodes).length > 0;
    const hasEdges = update.edges && Object.keys(update.edges).length > 0;
    const hasGanttTasks = update.ganttTasks && Object.keys(update.ganttTasks).length > 0;
    if (!hasNodes && !hasEdges && !hasGanttTasks) {
      return;
    }
    void applyUpdate(update);
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

const handleSelectNodes = useCallback((ids: string[]) => {
  setSelectedNodeIds(ids);
  setSelectedNodeId(ids.length === 1 ? ids[0] : null);
  if (ids.length > 0) {
    setSelectedEdgeId(null);
  }
}, []);

const handleSelectEdge = useCallback((id: string | null) => {
  setSelectedEdgeId(id);
  if (id) {
    setSelectedNodeId(null);
    setSelectedNodeIds([]);
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
        setSelectedNodeIds((current) => current.filter((id) => id !== target.id));
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

  const resolveCodeMapPathCandidates = useCallback((rawTarget: string) => {
    const trimmed = rawTarget.trim().replace(/^`|`$/g, "");
    const withoutQuery = trimmed.split("?")[0] ?? trimmed;
    const normalized = withoutQuery.replace(/^\.\//, "");

    const candidates: string[] = [];
    const add = (value: string) => {
      const cleaned = value.replace(/^\.\//, "").replace(/^\//, "");
      if (!cleaned || cleaned.includes("..")) {
        return;
      }
      if (!candidates.includes(cleaned)) {
        candidates.push(cleaned);
      }
    };

    // Strip leading ../ segments (server forbids directory traversal).
    let stripped = normalized;
    while (stripped.startsWith("../")) {
      stripped = stripped.slice(3);
    }
    add(stripped);

    // If the authored link looks like a frontend import, try rooting it at frontend/ too.
    if (!stripped.startsWith("frontend/") && !stripped.startsWith("src/") && !stripped.startsWith("tests/")) {
      add(`frontend/${stripped}`);
      add(`frontend/app/${stripped}`);
    }

    const hasExtension = /\.[a-zA-Z0-9]+$/.test(stripped);
    if (!hasExtension) {
      const exts = [".ts", ".tsx", ".js", ".jsx", ".rs", ".md"];
      for (const ext of exts) {
        add(`${stripped}${ext}`);
        add(`frontend/${stripped}${ext}`);
        add(`frontend/app/${stripped}${ext}`);
      }
    }

    return candidates;
  }, []);

  const navigateToCode = useCallback(
    (target: string, startLine?: number, endLine?: number, options?: { preserveMatches?: boolean }) => {
      const candidates = resolveCodeMapPathCandidates(target);
      const tryFetch = async () => {
        let lastError: unknown = null;
        for (const candidate of candidates) {
          try {
            const content = await fetchCodeMapFile(candidate);
            setSelectedFile({ path: candidate, content });
            if (!options?.preserveMatches) {
              setSearchResults(null);
              setActiveSearchIndex(0);
            }
            if (startLine && endLine) {
              setHighlightedLines({ start: startLine, end: endLine });
            } else if (startLine) {
              setHighlightedLines({ start: startLine, end: startLine });
            } else {
              setHighlightedLines(null);
            }
            return;
          } catch (err) {
            lastError = err;
          }
        }

        // Graceful failure: report once via the existing app error banner.
        setError(`Could not open referenced file: ${target}`);
        if (lastError) {
          // Keep details available for debugging without spamming console.
          void lastError;
        }
      };

      void tryFetch();
    },
    [resolveCodeMapPathCandidates]
  );

  const handleNavigate = useCallback(
    (target: string, startLine?: number, endLine?: number) => {
      const isFileLike = target.includes('/') || target.includes('.') || startLine !== undefined;

      if (isFileLike) {
        navigateToCode(target, startLine, endLine);
        return;
      }

      // Prefer the codemap-provided symbol location as the "definition" when present.
      const mapped = codeMapMapping
        ? Object.values(codeMapMapping.nodes).find((location) => location.symbol === target)
        : undefined;

      searchCodebase(target)
        .then((results) => {
          if (results.length === 0) {
            setError(`No matches found for: ${target}`);
            return;
          }

          const score = (content: string) => {
            const line = content;
            if (line.includes('export') && line.includes(target)) return 100;
            if (line.includes(`export async function ${target}`)) return 95;
            if (line.includes(`export function ${target}`)) return 90;
            if (line.includes(`async function ${target}`)) return 80;
            if (line.includes(`function ${target}`)) return 75;
            if (line.includes(`const ${target}`) && line.includes('=') && line.includes('=>')) return 70;
            if (line.includes(`fn ${target}`)) return 70;
            return 1;
          };

          let bestIndex = 0;
          let bestScore = -1;
          for (let i = 0; i < results.length; i += 1) {
            const current = results[i];
            let currentScore = score(current.content);
            if (mapped && current.file === mapped.file) {
              currentScore += 25;
              if (mapped.start_line && current.line === mapped.start_line) {
                currentScore += 50;
              }
            }
            if (currentScore > bestScore) {
              bestScore = currentScore;
              bestIndex = i;
            }
          }

          setSearchResults(results);
          setActiveSearchIndex(bestIndex);
          const best = results[bestIndex];
          navigateToCode(best.file, best.line, best.line, { preserveMatches: true });
        })
        .catch(() => setError(`Search failed for: ${target}`));
    },
    [navigateToCode, codeMapMapping]
  );

  const handleResultClick = useCallback((result: SearchResult) => {
    navigateToCode(result.file, result.line, result.line);
  }, [navigateToCode]);

  const handlePrevMatch = useCallback(() => {
    if (!searchResults || searchResults.length === 0) {
      return;
    }
    const nextIndex = (activeSearchIndex - 1 + searchResults.length) % searchResults.length;
    setActiveSearchIndex(nextIndex);
    const next = searchResults[nextIndex];
    navigateToCode(next.file, next.line, next.line, { preserveMatches: true });
  }, [activeSearchIndex, navigateToCode, searchResults]);

  const handleNextMatch = useCallback(() => {
    if (!searchResults || searchResults.length === 0) {
      return;
    }
    const nextIndex = (activeSearchIndex + 1) % searchResults.length;
    setActiveSearchIndex(nextIndex);
    const next = searchResults[nextIndex];
    navigateToCode(next.file, next.line, next.line, { preserveMatches: true });
  }, [activeSearchIndex, navigateToCode, searchResults]);

const handleDeleteSelection = useCallback(async () => {
  if (saving || sourceSaving) {
    return;
  }
  if (selectedNodeIds.length > 0) {
    try {
      setSaving(true);
      setError(null);
      for (const nodeId of selectedNodeIds) {
        await deleteNode(nodeId);
      }
      setSelectedNodeIds([]);
      setSelectedNodeId(null);
      setSelectedEdgeId(null);
      await loadDiagram({ silent: true });
    } catch (err) {
      setError((err as Error).message);
    } finally {
      setSaving(false);
    }
  } else if (selectedNodeId) {
    await deleteTarget({ type: "node", id: selectedNodeId });
  } else if (selectedEdgeId) {
    await deleteTarget({ type: "edge", id: selectedEdgeId });
  }
}, [deleteNode, deleteTarget, loadDiagram, saving, selectedEdgeId, selectedNodeId, selectedNodeIds, sourceSaving]);

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

const handleDownloadPng = useCallback(async () => {
  if (!diagram || !svgMarkup || downloadingPng) {
    return;
  }

  try {
    setDownloadingPng(true);
    setError(null);
    const pngBlob = await svgMarkupToPngBlob(svgMarkup);
    downloadBlob(pngBlob, toPngFilename(diagram.sourcePath));
  } catch (err) {
    setError((err as Error).message);
  } finally {
    setDownloadingPng(false);
  }
}, [diagram, downloadingPng, svgMarkup]);

const handleShare = useCallback(async () => {
  if (!LOCAL_MODE || codedownMode || sharingLink) {
    return;
  }

  try {
    setSharingLink(true);
    setError(null);
    const shareUrl = await buildLocalShareUrl(sourceDraft);
    await copyTextToClipboard(shareUrl);
    setShareCopied(true);
    if (shareCopiedTimer.current !== null) {
      window.clearTimeout(shareCopiedTimer.current);
    }
    shareCopiedTimer.current = window.setTimeout(() => {
      setShareCopied(false);
      shareCopiedTimer.current = null;
    }, 2000);
  } catch (err) {
    setError((err as Error).message);
  } finally {
    setSharingLink(false);
  }
}, [codedownMode, sharingLink, sourceDraft]);

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
  if (downloadingPng) {
    return "Preparing PNG download...";
  }
  if (sharingLink) {
    return "Preparing share link...";
  }
  if (shareCopied) {
    return "Share link copied.";
  }
  if (error) {
    return `Error: ${error}`;
  }
  return diagram ? `Editing ${diagram.sourcePath}` : "No diagram selected";
}, [diagram, downloadingPng, error, loading, saving, shareCopied, sharingLink, sourceSaving]);

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
  if (selectedNodeIds.length > 1) {
    return `Selected nodes: ${selectedNodeIds.length}`;
  }
  if (selectedNodeId) {
    return `Selected node: ${selectedNodeId}`;
  }
  if (selectedEdgeId) {
    return `Selected edge: ${selectedEdgeId}`;
  }
  if (activeTool === "connect" && connectStartNodeId) {
    return `Connect from: ${connectStartNodeId}`;
  }
  if (activeTool === "add-node" && addNodeSourceId) {
    return `Add from: ${addNodeSourceId}`;
  }
  return "No selection";
}, [activeTool, addNodeSourceId, connectStartNodeId, selectedEdgeId, selectedNodeId, selectedNodeIds]);

const handleCanvasAddNode = useCallback((point: Point) => {
  if (!diagram) {
    return;
  }
  try {
    const label = `Node ${diagram.nodes.length + reservedNodeIdsRef.current.size + 1}`;
    const id = makeUniqueNodeId(
      [...diagram.nodes.map((node) => node.id), ...Array.from(reservedNodeIdsRef.current)],
      label
    );
    reservedNodeIdsRef.current.add(id);
    const nextSource = addNodeToSource(sourceDraft, {
      id,
      label,
      shape: toolNodeShape,
    });
    const finalSource = addNodeSourceId
      ? addEdgeToSource(nextSource, {
          from: addNodeSourceId,
          to: id,
          label: "",
          kind: toolEdgeKind,
          directed: toolEdgeDirected,
        })
      : nextSource;
    lastSubmittedSource.current = null;
    setSourceDraft(finalSource);
    setPendingPlacedNode({ id, point });
    setSelectedNodeId(id);
    setSelectedNodeIds([id]);
    setSelectedEdgeId(null);
    setError(null);
    setSourceError(null);
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to add node.";
    setSourceError(message);
    setError(message);
  }
}, [addNodeSourceId, diagram, sourceDraft, toolEdgeDirected, toolEdgeKind, toolNodeShape]);

const handleConnectNodeClick = useCallback((nodeId: string) => {
  if (!connectStartNodeId) {
    setConnectStartNodeId(nodeId);
    return;
  }
  if (connectStartNodeId === nodeId) {
    setConnectStartNodeId(null);
    return;
  }
  try {
    const nextSource = addEdgeToSource(sourceDraft, {
      from: connectStartNodeId,
      to: nodeId,
      label: "",
      kind: toolEdgeKind,
      directed: toolEdgeDirected,
    });
    lastSubmittedSource.current = null;
    setSourceDraft(nextSource);
    setConnectStartNodeId(nodeId);
    setSelectedNodeId(nodeId);
    setSelectedNodeIds([nodeId]);
    setSelectedEdgeId(null);
    setError(null);
    setSourceError(null);
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to add relation.";
    setSourceError(message);
    setError(message);
  }
}, [connectStartNodeId, sourceDraft, toolEdgeDirected, toolEdgeKind]);

const handleContextMenuRequest = useCallback((request: CanvasContextMenuRequest) => {
  setContextMenu(request);
}, []);

const handleNodeShapeChangeFromMenu = useCallback((shape: NodeShape) => {
  if (!contextMenu?.nodeId || !diagram) {
    return;
  }
  const node = diagram.nodes.find((entry) => entry.id === contextMenu.nodeId);
  if (!node) {
    return;
  }
  try {
    const nextSource = updateNodeShapeInSource(sourceDraft, node.id, node.label, shape);
    lastSubmittedSource.current = null;
    setSourceDraft(nextSource);
    setContextMenu(null);
    setError(null);
    setSourceError(null);
  } catch (err) {
    const message = err instanceof Error ? err.message : "Failed to change node shape.";
    setSourceError(message);
    setError(message);
  }
}, [contextMenu, diagram, sourceDraft]);

const handleEdgeTypeChangeFromMenu = useCallback((kind: EdgeKind, directed: boolean) => {
  if (!contextMenu?.edgeId) {
    return;
  }
  void submitStyleUpdate({
    edgeStyles: {
      [contextMenu.edgeId]: {
        line: kind,
        arrow: directed ? "forward" : "none",
      },
    },
  });
  setContextMenu(null);
}, [contextMenu, submitStyleUpdate]);

useEffect(() => {
  if (!diagram || !pendingPlacedNode) {
    return;
  }
  if (!diagram.nodes.some((node) => node.id === pendingPlacedNode.id)) {
    return;
  }
  handleNodeMove(pendingPlacedNode.id, pendingPlacedNode.point);
  setPendingPlacedNode(null);
}, [diagram, handleNodeMove, pendingPlacedNode]);

const hasAnySelection = selectedNodeIds.length > 0 || selectedNodeId !== null || selectedEdgeId !== null;
const ganttStyle = diagram?.kind === "gantt" ? diagram.gantt?.style : null;

const nodeFillValue = useMemo(() => {
  if (!selectedNode) {
    return (ganttStyle?.taskFill ?? DEFAULT_NODE_COLORS.rectangle).toLowerCase();
  }
  const ganttFallback = selectedNode.shape === "double-circle"
    ? ganttStyle?.milestoneFill
    : ganttStyle?.taskFill;
  return resolveColor(
    selectedNode.fillColor,
    ganttFallback ?? DEFAULT_NODE_COLORS[selectedNode.shape]
  );
}, [ganttStyle, selectedNode]);

const nodeStrokeValue = useMemo(() => {
  if (!selectedNode) {
    return DEFAULT_EDGE_COLOR.toLowerCase();
  }
  return resolveColor(selectedNode.strokeColor, DEFAULT_EDGE_COLOR);
}, [selectedNode]);

const nodeTextValue = useMemo(() => {
  if (!selectedNode) {
    return (ganttStyle?.taskText ?? DEFAULT_NODE_TEXT).toLowerCase();
  }
  const ganttFallback = selectedNode.shape === "double-circle"
    ? ganttStyle?.milestoneText
    : ganttStyle?.taskText;
  return resolveColor(selectedNode.textColor, ganttFallback ?? DEFAULT_NODE_TEXT);
}, [ganttStyle, selectedNode]);

const ganttRowEvenValue = useMemo(() => {
  return resolveColor(ganttStyle?.rowFillEven, "#eff6ff");
}, [ganttStyle?.rowFillEven]);

const ganttRowOddValue = useMemo(() => {
  return resolveColor(ganttStyle?.rowFillOdd, "#dbeafe");
}, [ganttStyle?.rowFillOdd]);

const ganttTaskFillValue = useMemo(() => {
  return resolveColor(ganttStyle?.taskFill, "#2563eb");
}, [ganttStyle?.taskFill]);

const ganttMilestoneFillValue = useMemo(() => {
  return resolveColor(ganttStyle?.milestoneFill, "#1d4ed8");
}, [ganttStyle?.milestoneFill]);

const ganttTaskTextValue = useMemo(() => {
  return resolveColor(ganttStyle?.taskText, "#ffffff");
}, [ganttStyle?.taskText]);

const ganttMilestoneTextValue = useMemo(() => {
  return resolveColor(ganttStyle?.milestoneText, "#111827");
}, [ganttStyle?.milestoneText]);

const nodeLabelFillValue = useMemo(() => {
  if (!selectedNode) {
    return nodeFillValue;
  }
  const fallback = selectedNode.image
    ? resolveColor(selectedNode.fillColor, DEFAULT_NODE_COLORS[selectedNode.shape])
    : nodeFillValue;
  return resolveColor(selectedNode.labelFillColor, fallback);
}, [selectedNode, nodeFillValue]);

const nodeImageFillValue = useMemo(() => {
  if (!selectedNode) {
    return nodeFillValue;
  }
  if (!selectedNode.image) {
    return nodeFillValue;
  }
  return resolveColor(selectedNode.imageFillColor, "#ffffff");
}, [selectedNode, nodeFillValue]);

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

const handleLineClick = useCallback((line: number) => {
  if (!codeMapMapping || !selectedFile) return;

  // Find the most specific node that covers this line
  let bestNodeId: string | null = null;
  let bestRangeSize = Infinity;

  for (const [nodeId, location] of Object.entries(codeMapMapping.nodes)) {
    if (location.file === selectedFile.path &&
      location.start_line !== undefined &&
      location.end_line !== undefined) {

      if (line >= location.start_line && line <= location.end_line) {
        const rangeSize = location.end_line - location.start_line;
        if (rangeSize < bestRangeSize) {
          bestRangeSize = rangeSize;
          bestNodeId = nodeId;
        }
      }
    }
  }

  if (bestNodeId) {
    handleSelectNode(bestNodeId);
  }
}, [codeMapMapping, selectedFile, handleSelectNode]);

return (
  <div className="app">
    <header className="toolbar">
      <div className="status" role="status" aria-live="polite">
        {statusMessage}
      </div>
      <div className="actions">
        <button onClick={toggleTheme} title="Toggle Theme">
          {theme === "light" ? "Dark Mode" : "Light Mode"}
        </button>
        <button
          onClick={() => void handleDownloadPng()}
          disabled={codedownMode || !diagram || !svgMarkup || loading || saving || sourceSaving || downloadingPng}
          title="Download the current diagram as a PNG"
        >
          {downloadingPng ? "Downloading PNG..." : "Download PNG"}
        </button>
        {LOCAL_MODE && (
          <button
            onClick={() => void handleShare()}
            disabled={codedownMode || !diagram || loading || saving || sourceSaving || sharingLink}
            title="Copy a shareable link for the current diagram"
          >
            {sharingLink ? "Sharing..." : shareCopied ? "Link Copied" : "Share"}
          </button>
        )}
        {isCodeAnnotated && codeMapMapping && (
          <button
            onClick={() => setCodeMapMode((current) => !current)}
            title="Toggle Code Map Mode"
          >
            {codeMapMode ? "Edit Diagram" : "View Code Map"}
          </button>
        )}
        <button
          onClick={handleResetOverrides}
          disabled={!hasOverrides(diagram) || saving || sourceSaving}
          title="Remove all manual positions"
        >
          Reset overrides
        </button>
        <button
          onClick={() => void handleDeleteSelection()}
          disabled={!hasAnySelection || saving || sourceSaving}
          title="Delete the currently selected node or edge"
        >
          Delete selected
        </button>
      </div>
    </header>
    <main className="workspace">
      {diagram && !loading ? (
        <>
          {isLeftPanelCollapsed ? (
            <div className="collapse-button collapsed-left" onClick={() => setIsLeftPanelCollapsed(false)} title="Expand Style Panel">
              ›
            </div>
          ) : (
              <aside className="style-panel" style={{ width: leftPanelWidth, maxWidth: "none" }}>
                <div
                  className="resize-handle"
                  onMouseDown={startLeftPanelResizing}
                  style={{
                    position: "absolute",
                    right: 0,
                    top: 0,
                    bottom: 0,
                    width: "8px",
                    cursor: "col-resize",
                    zIndex: 10,
                    background: "transparent",
                  }}
                />
                <button className="collapse-button left" onClick={() => setIsLeftPanelCollapsed(true)} title="Collapse Style Panel">
                  ‹
                </button>
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
                {!codedownMode && diagram.kind === "flowchart" ? (
                  <section className="toolbox-panel">
                    <div className="toolbox-group">
                      <button
                        type="button"
                        className={activeTool === "select" ? "toolbox-button active" : "toolbox-button"}
                        onClick={() => {
                          setActiveTool("select");
                          setConnectStartNodeId(null);
                        }}
                      >
                        Select
                      </button>
                      <button
                        type="button"
                        className={activeTool === "add-node" ? "toolbox-button active" : "toolbox-button"}
                        onClick={() => {
                          setActiveTool("add-node");
                          setConnectStartNodeId(null);
                        }}
                      >
                        Add Node
                      </button>
                      <button
                        type="button"
                        className={activeTool === "connect" ? "toolbox-button active" : "toolbox-button"}
                        onClick={() => setActiveTool("connect")}
                      >
                        Connect
                      </button>
                    </div>
                    {activeTool === "add-node" ? (
                      <div className="toolbox-group">
                        <label className="toolbox-field">
                          <span>Node shape</span>
                          <select
                            value={toolNodeShape}
                            onChange={(event) => setToolNodeShape(event.target.value as NodeShape)}
                          >
                            {NODE_SHAPE_OPTIONS.map((option) => (
                              <option key={option.value} value={option.value}>
                                {option.label}
                              </option>
                            ))}
                          </select>
                        </label>
                      </div>
                    ) : null}
                    {activeTool === "connect" ? (
                      <div className="toolbox-group">
                        <label className="toolbox-field">
                          <span>Line</span>
                          <select
                            value={toolEdgeKind}
                            onChange={(event) => setToolEdgeKind(event.target.value as EdgeKind)}
                          >
                            {LINE_STYLE_OPTIONS.map((option) => (
                              <option key={option.value} value={option.value}>
                                {option.label}
                              </option>
                            ))}
                          </select>
                        </label>
                        <label className="toolbox-toggle">
                          <input
                            type="checkbox"
                            checked={toolEdgeDirected}
                            onChange={(event) => setToolEdgeDirected(event.target.checked)}
                          />
                          <span>Arrow</span>
                        </label>
                      </div>
                    ) : null}
                    <div className="toolbox-group">
                      <div className="toolbox-zoom-row">
                        <button type="button" className="toolbox-button" onClick={() => viewportControls?.zoomOut()}>
                          -
                        </button>
                        <button
                          type="button"
                          className="toolbox-button toolbox-zoom-display"
                          onClick={() => viewportControls?.resetZoom()}
                        >
                          {viewportControls ? `${Math.round(viewportControls.getScale() * 100)}%` : "100%"}
                        </button>
                        <button type="button" className="toolbox-button" onClick={() => viewportControls?.zoomIn()}>
                          +
                        </button>
                      </div>
                    </div>
                  </section>
                ) : null}
                {diagram.kind === "gantt" && diagram.gantt ? (
                  <section className="style-section">
                    <header className="section-heading">
                      <h3>Gantt</h3>
                      <span className="section-caption">Timeline styles</span>
                    </header>
                    <div className="style-controls">
                      <div className="style-color-row">
                        <label className="style-control">
                          <span>Row even</span>
                          <input
                            type="color"
                            value={ganttRowEvenValue}
                            onChange={(event) => handleGanttStyleChange("rowFillEven", event.target.value)}
                            disabled={saving}
                          />
                        </label>
                        <label className="style-control">
                          <span>Row odd</span>
                          <input
                            type="color"
                            value={ganttRowOddValue}
                            onChange={(event) => handleGanttStyleChange("rowFillOdd", event.target.value)}
                            disabled={saving}
                          />
                        </label>
                        <label className="style-control">
                          <span>Task fill</span>
                          <input
                            type="color"
                            value={ganttTaskFillValue}
                            onChange={(event) => handleGanttStyleChange("taskFill", event.target.value)}
                            disabled={saving}
                          />
                        </label>
                        <label className="style-control">
                          <span>Task text</span>
                          <input
                            type="color"
                            value={ganttTaskTextValue}
                            onChange={(event) => handleGanttStyleChange("taskText", event.target.value)}
                            disabled={saving}
                          />
                        </label>
                        <label className="style-control">
                          <span>Milestone fill</span>
                          <input
                            type="color"
                            value={ganttMilestoneFillValue}
                            onChange={(event) => handleGanttStyleChange("milestoneFill", event.target.value)}
                            disabled={saving}
                          />
                        </label>
                        <label className="style-control">
                          <span>Milestone text</span>
                          <input
                            type="color"
                            value={ganttMilestoneTextValue}
                            onChange={(event) => handleGanttStyleChange("milestoneText", event.target.value)}
                            disabled={saving}
                          />
                        </label>
                      </div>
                    </div>
                  </section>
                ) : null}
                <section className="style-section">
                  <header className="section-heading">
                    <h3>Node</h3>
                    <span className={selectedNode ? "section-caption" : "section-caption muted"}>
                      {selectedNode ? selectedNode.label || selectedNode.id : "No node selected"}
                    </span>
                  </header>
                  <div className="style-controls" aria-disabled={nodeControlsDisabled}>
                    <div className="style-color-row">
                      {!selectedNode?.image ? (
                        <label className="style-control">
                          <span>Fill</span>
                          <input
                            type="color"
                            value={nodeFillValue}
                            onChange={(event) => handleNodeFillChange(event.target.value)}
                            disabled={nodeControlsDisabled}
                          />
                        </label>
                      ) : null}
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
                      {selectedNode?.image ? (
                        <>
                          <label className="style-control">
                            <span>Title background</span>
                            <input
                              type="color"
                              value={nodeLabelFillValue}
                              onChange={(event) => handleNodeLabelFillChange(event.target.value)}
                              disabled={nodeControlsDisabled}
                            />
                          </label>
                          <label className="style-control">
                            <span>Image background</span>
                            <input
                              type="color"
                              value={nodeImageFillValue}
                              onChange={(event) => handleNodeImageFillChange(event.target.value)}
                              disabled={nodeControlsDisabled}
                            />
                          </label>
                        </>
                      ) : null}
                    </div>
                    <div className="style-control image-control">
                      <span>Image</span>
                      <div className="image-control-actions">
                        <button
                          type="button"
                          onClick={() => nodeImageInputRef.current?.click()}
                          disabled={nodeControlsDisabled}
                        >
                          {selectedNode?.image ? "Replace PNG" : "Upload PNG"}
                        </button>
                        <button
                          type="button"
                          onClick={() => void handleNodeImageRemove()}
                          disabled={nodeControlsDisabled || !selectedNode?.image}
                        >
                          Remove
                        </button>
                      </div>
                      <input
                        ref={nodeImageInputRef}
                        type="file"
                        accept="image/png"
                        onChange={handleNodeImageFileChange}
                        hidden
                      />
                  <span
                        className="image-control-meta"
                      >
                        {selectedNode?.image
                          ? `${selectedNode.image.width}x${selectedNode.image.height}px (padding ${formatPaddingValue(
                            selectedNode.image.padding
                          )}px)`
                          : ""}
                      </span>
                    </div>
                    <label className="style-control">
                      <span>Image padding (px)</span>
                      <input
                        type="number"
                        min={0}
                        step={1}
                        inputMode="decimal"
                        value={imagePaddingValue}
                        onChange={(event) => handleNodeImagePaddingChange(event.target.value)}
                        onBlur={handleNodeImagePaddingBlur}
                        onKeyDown={(event) => handleNodeImagePaddingKeyDown(event)}
                        disabled={nodeControlsDisabled || !selectedNode?.image}
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
                  {codeMapMode && selectedNode && codeMapMapping?.nodes[selectedNode.id] && (
                    <div className="editor-link-actions" style={{ marginTop: "0.5rem" }}>
                      <button
                        type="button"
                        className="editor-link-button"
                        onClick={() => {
                          const loc = codeMapMapping.nodes[selectedNode.id];
                          void openInEditor(loc.file, loc.start_line, "vscode");
                        }}
                      >
                        Open in VS Code
                      </button>
                      <button
                        type="button"
                        className="editor-link-button"
                        onClick={() => {
                          const loc = codeMapMapping.nodes[selectedNode.id];
                          void openInEditor(loc.file, loc.start_line, "nvim");
                        }}
                      >
                        Open in Vi
                      </button>
                    </div>
                  )}
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
          )}
            {codedownMode ? (
              <MarkdownViewer
                content={markdownContent}
                onNavigate={handleNavigate}
                codeMapMapping={codeMapMapping}
              />
            ) : (
              <WasmDiagramCanvas
                diagram={diagram}
                onNodeMove={handleNodeMove}
                onLayoutUpdate={handleLayoutUpdate}
                onEdgeMove={handleEdgeMove}
                onSvgMarkupChange={setSvgMarkup}
                selectedNodeId={selectedNodeId}
                selectedNodeIds={selectedNodeIds}
                selectedEdgeId={selectedEdgeId}
                onSelectNode={handleSelectNode}
                onSelectNodes={handleSelectNodes}
                onSelectEdge={handleSelectEdge}
                onDragStateChange={setDragging}
                onDeleteNode={handleDeleteNodeDirect}
                onDeleteEdge={handleDeleteEdgeDirect}
                codeMapMapping={codeMapMapping}
                activeTool={activeTool}
                connectStartNodeId={connectStartNodeId}
                addNodeSourceId={addNodeSourceId}
                toolNodeShape={toolNodeShape}
                toolEdgeKind={toolEdgeKind}
                toolEdgeDirected={toolEdgeDirected}
                onCanvasAddNode={handleCanvasAddNode}
                onAddNodeSourceSelect={setAddNodeSourceId}
                onConnectNodeClick={handleConnectNodeClick}
                onContextMenuRequest={handleContextMenuRequest}
                onViewportControlsChange={setViewportControls}
              />
            )}
            {(codeMapMode || codedownMode) ? (
            <CodePanel
              filePath={selectedFile?.path ?? null}
              content={selectedFile?.content ?? null}
              startLine={highlightedLines?.start}
              endLine={highlightedLines?.end}
              onClose={() => { setSelectedFile(null); setSearchResults(null); setActiveSearchIndex(0); }}
              onLineClick={handleLineClick}
              matchInfo={searchResults ? { index: activeSearchIndex, total: searchResults.length } : null}
              onPrevMatch={handlePrevMatch}
              onNextMatch={handleNextMatch}
            />
          ) : (
            isRightPanelCollapsed ? (
              <div className="collapse-button collapsed-right" onClick={() => setIsRightPanelCollapsed(false)} title="Expand Source Panel">
                ‹
              </div>
            ) : (
              <aside className="source-panel" style={{ width: rightPanelWidth, maxWidth: "none" }}>
                <div
                  className="resize-handle"
                  onMouseDown={startRightPanelResizing}
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
                <button className="collapse-button right" onClick={() => setIsRightPanelCollapsed(true)} title="Collapse Source Panel">
                  ›
                </button>
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
            )
          )}
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
    {contextMenu ? (
      <div
        className="context-menu"
        style={{ left: contextMenu.clientX, top: contextMenu.clientY }}
        onPointerDown={(event) => event.stopPropagation()}
      >
        {contextMenu.kind === "canvas" && contextMenu.point ? (
          <button
            type="button"
            onClick={() => {
              handleCanvasAddNode(contextMenu.point as Point);
              setContextMenu(null);
            }}
          >
            Add {NODE_SHAPE_OPTIONS.find((option) => option.value === toolNodeShape)?.label ?? "Node"} Here
          </button>
        ) : null}
        {contextMenu.kind === "node" ? (
          <>
            {NODE_SHAPE_OPTIONS.map((option) => (
              <button
                key={option.value}
                type="button"
                onClick={() => handleNodeShapeChangeFromMenu(option.value)}
              >
                Shape: {option.label}
              </button>
            ))}
            {contextMenu.nodeId ? <div className="context-menu-separator" /> : null}
            {contextMenu.nodeId ? (
              <button type="button" onClick={() => void handleDeleteNodeDirect(contextMenu.nodeId!)}>
                Delete Node
              </button>
            ) : null}
          </>
        ) : null}
        {contextMenu.kind === "edge" ? (
          <>
            <button type="button" onClick={() => handleEdgeTypeChangeFromMenu("solid", true)}>
              Relation: Solid Arrow
            </button>
            <button type="button" onClick={() => handleEdgeTypeChangeFromMenu("dashed", true)}>
              Relation: Dashed Arrow
            </button>
            <button type="button" onClick={() => handleEdgeTypeChangeFromMenu("solid", false)}>
              Relation: Plain Line
            </button>
            <div className="context-menu-separator" />
            {contextMenu.edgeId ? (
              <button type="button" onClick={() => void handleDeleteEdgeDirect(contextMenu.edgeId!)}>
                Delete Relation
              </button>
            ) : null}
          </>
        ) : null}
      </div>
    ) : null}
  </div>
);
}
