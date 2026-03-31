import type { EdgeKind, NodeShape } from "./types";

const LAYOUT_BLOCK_START = "%% OXDRAW LAYOUT START";
const LAYOUT_BLOCK_END = "%% OXDRAW LAYOUT END";

export interface SourceNodeOption {
  id: string;
  label: string;
}

export interface AddNodeInput {
  id: string;
  label: string;
  shape: NodeShape;
}

export interface AddEdgeInput {
  from: string;
  to: string;
  label: string;
  kind: EdgeKind;
  directed: boolean;
}

interface SourceSections {
  definition: string;
  layoutBlock: string;
}

interface ParsedNodeSpec {
  id: string;
  label: string;
}

interface ParsedEdgeSpec {
  indent: string;
  from: string;
  to: string;
  label: string;
  token: string;
}

const EDGE_PATTERNS: Array<{ token: string }> = [{ token: "-.->" }, { token: "-->" }, { token: "---" }];
const NODE_ID_RE = /^[A-Za-z_][A-Za-z0-9_-]*$/;

function normalizeSource(source: string): string {
  return source.replace(/\r\n/g, "\n");
}

function splitSourceSections(source: string): SourceSections {
  const normalized = normalizeSource(source);
  const lines = normalized.split("\n");
  const definitionLines: string[] = [];
  const layoutLines: string[] = [];
  let inLayoutBlock = false;

  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed === LAYOUT_BLOCK_START) {
      inLayoutBlock = true;
      layoutLines.push(line);
      continue;
    }
    if (trimmed === LAYOUT_BLOCK_END) {
      layoutLines.push(line);
      inLayoutBlock = false;
      continue;
    }
    if (inLayoutBlock) {
      layoutLines.push(line);
    } else {
      definitionLines.push(line);
    }
  }

  return {
    definition: definitionLines.join("\n"),
    layoutBlock: layoutLines.join("\n").trim(),
  };
}

function joinSourceSections(definition: string, layoutBlock: string): string {
  const trimmedDefinition = definition.trimEnd();
  const trimmedLayout = layoutBlock.trim();
  if (!trimmedLayout) {
    return `${trimmedDefinition}\n`;
  }
  return `${trimmedDefinition}\n\n${trimmedLayout}\n`;
}

function parseNodeSpec(raw: string): ParsedNodeSpec {
  const trimmed = raw.trim();
  if (!trimmed) {
    throw new Error("Node reference cannot be empty.");
  }

  let idEnd = trimmed.length;
  for (let index = 0; index < trimmed.length; index += 1) {
    const char = trimmed[index];
    if (char === "[" || char === "(" || char === "{" || char === ">") {
      idEnd = index;
      break;
    }
  }

  const id = trimmed.slice(0, idEnd).trim();
  if (!id) {
    throw new Error(`Invalid node reference: ${raw}`);
  }

  const remainder = trimmed.slice(idEnd).trim();
  if (!remainder) {
    return { id, label: id };
  }

  const parsedLabel = parseShapeLabel(remainder);
  return {
    id,
    label: parsedLabel || id,
  };
}

function parseShapeLabel(spec: string): string | null {
  const trimmed = spec.trim();
  if (!trimmed) {
    return null;
  }

  const enclosingPatterns: Array<{ start: string; end: string }> = [
    { start: "(((", end: ")))" },
    { start: "((", end: "))" },
    { start: "[[", end: "]]" },
    { start: "[(", end: ")]" },
    { start: "{{", end: "}}" },
    { start: "[/", end: "/]" },
    { start: "[\\", end: "\\]" },
    { start: "[/", end: "\\]" },
    { start: "[\\", end: "/]" },
    { start: "[", end: "]" },
    { start: "(", end: ")" },
    { start: "{", end: "}" },
  ];

  for (const pattern of enclosingPatterns) {
    if (trimmed.startsWith(pattern.start) && trimmed.endsWith(pattern.end)) {
      return trimmed.slice(pattern.start.length, trimmed.length - pattern.end.length).trim();
    }
  }

  if (trimmed.startsWith(">") && trimmed.endsWith("]")) {
    return trimmed.slice(1, -1).trim();
  }

  return null;
}

function parseEdgeNodes(line: string): ParsedNodeSpec[] {
  const edge = parseEdgeSpec(line);
  if (!edge) {
    return [];
  }
  return [parseNodeSpec(edge.from), parseNodeSpec(edge.to)];
}

function parseEdgeSpec(line: string): ParsedEdgeSpec | null {
  const indentMatch = line.match(/^(\s*)/);
  const indent = indentMatch?.[1] ?? "";
  const trimmed = line.trim().replace(/;$/, "");
  for (const pattern of EDGE_PATTERNS) {
    const splitIndex = trimmed.indexOf(pattern.token);
    if (splitIndex === -1) {
      continue;
    }

    const left = trimmed.slice(0, splitIndex).trim();
    let right = trimmed.slice(splitIndex + pattern.token.length).trim();
    let label = "";
    if (right.startsWith("|")) {
      const closingIndex = right.indexOf("|", 1);
      if (closingIndex !== -1) {
        label = right.slice(1, closingIndex).trim();
        right = right.slice(closingIndex + 1).trim();
      }
    }

    return { indent, from: left, to: right, label, token: pattern.token };
  }

  return null;
}

function formatEdgeLine(spec: ParsedEdgeSpec): string {
  if (spec.label) {
    return `${spec.indent}${spec.from} ${spec.token}|${spec.label}| ${spec.to}`;
  }
  return `${spec.indent}${spec.from} ${spec.token} ${spec.to}`;
}

function collectNodeOptions(definition: string): SourceNodeOption[] {
  const nodes = new Map<string, string>();
  const lines = definition.split("\n");

  for (const rawLine of lines) {
    const line = rawLine.trim().replace(/;$/, "");
    if (!line || line.startsWith("%%")) {
      continue;
    }
    if (line.startsWith("graph ") || line.startsWith("flowchart ") || line === "end" || line.startsWith("subgraph ")) {
      continue;
    }

    const edgeNodes = parseEdgeNodes(line);
    if (edgeNodes.length > 0) {
      for (const node of edgeNodes) {
        if (!nodes.has(node.id)) {
          nodes.set(node.id, node.label);
        }
      }
      continue;
    }

    try {
      const node = parseNodeSpec(line);
      if (!nodes.has(node.id)) {
        nodes.set(node.id, node.label);
      }
    } catch {
      // Ignore lines outside the flowchart subset used by the quick editor.
    }
  }

  return Array.from(nodes.entries())
    .map(([id, label]) => ({ id, label }))
    .sort((a, b) => a.id.localeCompare(b.id));
}

function hasExistingEdge(definition: string, input: AddEdgeInput): boolean {
  const targetLabel = input.label.trim();
  const targetToken = input.kind === "dashed" ? "-.->" : input.directed ? "-->" : "---";

  for (const rawLine of definition.split("\n")) {
    const parsed = parseEdgeSpec(rawLine);
    if (!parsed) {
      continue;
    }

    const fromId = parseNodeSpec(parsed.from).id;
    const toId = parseNodeSpec(parsed.to).id;

    if (input.directed) {
      if (fromId === input.from && toId === input.to) {
        return true;
      }
      continue;
    }

    const samePair =
      (fromId === input.from && toId === input.to) ||
      (fromId === input.to && toId === input.from);
    if (samePair && (parsed.token === "---" || parsed.token === targetToken || !targetLabel)) {
      return true;
    }
  }

  return false;
}

function isEdgeLine(line: string): boolean {
  return EDGE_PATTERNS.some((pattern) => line.includes(pattern.token));
}

function formatNodeSpec({ id, label, shape }: AddNodeInput): string {
  switch (shape) {
    case "rectangle":
      return label === id ? id : `${id}[${label}]`;
    case "stadium":
      return `${id}(${label})`;
    case "circle":
      return `${id}((${label}))`;
    case "double-circle":
      return `${id}(((${label})))`;
    case "diamond":
      return `${id}{${label}}`;
    case "subroutine":
      return `${id}[[${label}]]`;
    case "cylinder":
      return `${id}[(${label})]`;
    case "hexagon":
      return `${id}{{${label}}}`;
    case "parallelogram":
      return `${id}[/${label}/]`;
    case "parallelogram-alt":
      return `${id}[\\${label}\\]`;
    case "trapezoid":
      return `${id}[/${label}\\]`;
    case "trapezoid-alt":
      return `${id}[\\${label}/]`;
    case "asymmetric":
      return `${id}>${label}]`;
    default:
      return `${id}[${label}]`;
  }
}

function formatEdgeSpec({ from, to, label, kind, directed }: AddEdgeInput): string {
  const connector = kind === "dashed" ? "-.->" : directed ? "-->" : "---";
  if (!label.trim()) {
    return `${from} ${connector} ${to}`;
  }
  return `${from} ${connector}|${label.trim()}| ${to}`;
}

function assertFlowchartDefinition(definition: string): void {
  const firstMeaningfulLine = definition
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line.length > 0);

  if (!firstMeaningfulLine) {
    throw new Error("Quick add needs an existing flowchart header.");
  }
  if (!/^graph\b/i.test(firstMeaningfulLine) && !/^flowchart\b/i.test(firstMeaningfulLine)) {
    throw new Error("Quick add currently supports flowcharts only.");
  }
}

export function slugifyNodeId(label: string): string {
  const slug = label
    .trim()
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "");

  if (!slug) {
    return "node";
  }
  if (/^[a-z_]/.test(slug)) {
    return slug;
  }
  return `node-${slug}`;
}

export function makeUniqueNodeId(existingIds: string[], baseLabel: string): string {
  const existing = new Set(existingIds);
  const base = slugifyNodeId(baseLabel);
  if (!existing.has(base)) {
    return base;
  }
  let suffix = 2;
  while (existing.has(`${base}-${suffix}`)) {
    suffix += 1;
  }
  return `${base}-${suffix}`;
}

export function getSourceNodeOptions(source: string): SourceNodeOption[] {
  const { definition } = splitSourceSections(source);
  return collectNodeOptions(definition);
}

export function addNodeToSource(source: string, input: AddNodeInput): string {
  const { definition, layoutBlock } = splitSourceSections(source);
  assertFlowchartDefinition(definition);

  const id = input.id.trim();
  const label = input.label.trim();

  if (!NODE_ID_RE.test(id)) {
    throw new Error("Node id must start with a letter or underscore and use letters, numbers, '-' or '_'.");
  }
  if (!label) {
    throw new Error("Node label is required.");
  }

  const existingNodes = collectNodeOptions(definition);
  if (existingNodes.some((node) => node.id === id)) {
    throw new Error(`Node '${id}' already exists.`);
  }

  const lines = definition.trimEnd().split("\n");
  const insertAt = lines.findIndex((line, index) => index > 0 && isEdgeLine(line.trim()));
  const nextLines = [...lines];
  const nodeLine = formatNodeSpec({ ...input, id, label });
  if (insertAt === -1) {
    if (nextLines[nextLines.length - 1]?.trim()) {
      nextLines.push("");
    }
    nextLines.push(nodeLine);
  } else {
    nextLines.splice(insertAt, 0, nodeLine);
  }

  return joinSourceSections(nextLines.join("\n"), layoutBlock);
}

export function addEdgeToSource(source: string, input: AddEdgeInput): string {
  const { definition, layoutBlock } = splitSourceSections(source);
  assertFlowchartDefinition(definition);

  const from = input.from.trim();
  const to = input.to.trim();
  if (!from || !to) {
    throw new Error("Select both source and target nodes.");
  }
  if (from === to) {
    throw new Error("Self-relations are not supported in quick add yet.");
  }

  const existingNodes = collectNodeOptions(definition);
  const nodeIds = new Set(existingNodes.map((node) => node.id));
  if (!nodeIds.has(from) || !nodeIds.has(to)) {
    throw new Error("Relation endpoints must refer to existing nodes.");
  }

  if (hasExistingEdge(definition, { ...input, from, to, label: input.label.trim() })) {
    return joinSourceSections(definition, layoutBlock);
  }

  const edgeLine = formatEdgeSpec({
    ...input,
    from,
    to,
    label: input.label.trim(),
  });

  const definitionBody = definition.trimEnd();
  const nextDefinition = definitionBody
    ? `${definitionBody}\n\n${edgeLine}`
    : edgeLine;

  return joinSourceSections(nextDefinition, layoutBlock);
}

export function updateNodeShapeInSource(
  source: string,
  nodeId: string,
  label: string,
  shape: NodeShape
): string {
  const { definition, layoutBlock } = splitSourceSections(source);
  assertFlowchartDefinition(definition);

  const lines = definition.split("\n");
  let replaced = false;

  const nextLines = lines.map((rawLine) => {
    const trimmed = rawLine.trim().replace(/;$/, "");
    if (!trimmed || trimmed.startsWith("%%") || trimmed === "end" || trimmed.startsWith("subgraph ")) {
      return rawLine;
    }
    if (/^graph\b/i.test(trimmed) || /^flowchart\b/i.test(trimmed)) {
      return rawLine;
    }

    const edge = parseEdgeSpec(rawLine);
    if (edge) {
      let changed = false;
      const fromSpec = parseNodeSpec(edge.from);
      const toSpec = parseNodeSpec(edge.to);
      if (fromSpec.id === nodeId) {
        edge.from = formatNodeSpec({ id: nodeId, label, shape });
        changed = true;
      }
      if (toSpec.id === nodeId) {
        edge.to = formatNodeSpec({ id: nodeId, label, shape });
        changed = true;
      }
      if (changed) {
        replaced = true;
        return formatEdgeLine(edge);
      }
      return rawLine;
    }

    try {
      const node = parseNodeSpec(trimmed);
      if (node.id !== nodeId) {
        return rawLine;
      }
      const indent = rawLine.match(/^(\s*)/)?.[1] ?? "";
      replaced = true;
      return `${indent}${formatNodeSpec({ id: nodeId, label, shape })}`;
    } catch {
      return rawLine;
    }
  });

  if (!replaced) {
    throw new Error(`Node '${nodeId}' was not found in the source.`);
  }

  return joinSourceSections(nextLines.join("\n"), layoutBlock);
}
