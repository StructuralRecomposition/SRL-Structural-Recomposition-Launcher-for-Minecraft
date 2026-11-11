import type { DependencyEdge, DependencyNode } from "./api";

export interface GraphNode {
  node: DependencyNode;
  x: number;
  y: number;
  width: number;
  height: number;
}

export interface GraphEdge {
  edge: DependencyEdge;
  from: GraphNode;
  to: GraphNode;
}

export interface GraphLayout {
  nodes: GraphNode[];
  edges: GraphEdge[];
  width: number;
  height: number;
}

export const NODE_WIDTH = 196;
export const NODE_HEIGHT = 62;
const COLUMN_GAP = 72;
const ROW_GAP = 22;
const PADDING = 24;

/**
 * Deterministic left-to-right layered layout. The depth field comes from the
 * resolver, not from a guess about the visual shape, so cyclic graphs and
 * multiple parents still render in stable columns.
 */
export function layoutDependencyGraph(
  nodes: DependencyNode[],
  edges: DependencyEdge[],
): GraphLayout {
  if (nodes.length === 0) {
    return { nodes: [], edges: [], width: 0, height: 0 };
  }
  const byDepth = new Map<number, DependencyNode[]>();
  for (const node of nodes) {
    const depth = Math.max(0, node.depth);
    const bucket = byDepth.get(depth);
    if (bucket) bucket.push(node);
    else byDepth.set(depth, [node]);
  }
  const depths = [...byDepth.keys()].sort((a, b) => a - b);
  const placed = new Map<string, GraphNode>();
  let maxRows = 0;
  for (const depth of depths) {
    const bucket = byDepth.get(depth) ?? [];
    bucket.sort((a, b) => {
      const parent = (a.parent ?? "").localeCompare(b.parent ?? "");
      if (parent !== 0) return parent;
      return a.title.localeCompare(b.title);
    });
    maxRows = Math.max(maxRows, bucket.length);
    bucket.forEach((node, index) => {
      placed.set(node.id, {
        node,
        x: PADDING + depth * (NODE_WIDTH + COLUMN_GAP),
        y: PADDING + index * (NODE_HEIGHT + ROW_GAP),
        width: NODE_WIDTH,
        height: NODE_HEIGHT,
      });
    });
  }
  const graphEdges: GraphEdge[] = [];
  for (const edge of edges) {
    const from = placed.get(edge.from);
    const to = placed.get(edge.to);
    if (from && to) graphEdges.push({ edge, from, to });
  }
  return {
    nodes: [...placed.values()],
    edges: graphEdges,
    width: PADDING * 2 + (depths.length - 1) * (NODE_WIDTH + COLUMN_GAP) + NODE_WIDTH,
    height: PADDING * 2 + Math.max(0, maxRows - 1) * (NODE_HEIGHT + ROW_GAP) + NODE_HEIGHT,
  };
}

export function edgePath(from: GraphNode, to: GraphNode): string {
  const x1 = from.x + from.width;
  const y1 = from.y + from.height / 2;
  const x2 = to.x;
  const y2 = to.y + to.height / 2;
  const mid = (x1 + x2) / 2;
  return `M ${x1} ${y1} C ${mid} ${y1}, ${mid} ${y2}, ${x2} ${y2}`;
}

export const RELATION_COLORS: Record<string, string> = {
  required: "#4f8cff",
  optional: "#9a7bff",
  incompatible: "#e35d6a",
  embedded: "#7a8494",
  tool: "#d49b3b",
  include: "#3fa67a",
  unknown: "#8b93a3",
};
