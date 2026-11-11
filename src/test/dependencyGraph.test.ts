import { describe, expect, it } from "vitest";
import type { DependencyEdge, DependencyNode } from "../lib/api";
import { edgePath, layoutDependencyGraph } from "../lib/dependencyGraph";

function node(id: string, depth: number, parent: string | null = null): DependencyNode {
  return {
    id,
    source: "modrinth",
    projectId: id,
    title: id,
    versionId: "v1",
    versionNumber: "1.0.0",
    versionType: "release",
    fileName: `${id}.jar`,
    url: null,
    sha1: null,
    sha512: null,
    size: 10,
    hasChecksum: true,
    relation: depth === 0 ? "required" : "required",
    depth,
    parent,
    requiredBy: parent ? [parent] : [],
    installed: false,
    installedFile: null,
    installedVersion: null,
    compatible: true,
    installable: true,
    selectedByDefault: true,
    notes: [],
  };
}

describe("dependency graph layout", () => {
  it("places nodes in depth columns and keeps every real edge", () => {
    const nodes = [node("root", 0), node("a", 1, "root"), node("b", 1, "root"), node("c", 2, "a")];
    const edges: DependencyEdge[] = [
      { from: "root", to: "a", relation: "required", rawType: "required" },
      { from: "root", to: "b", relation: "optional", rawType: "optional" },
      { from: "a", to: "c", relation: "required", rawType: "required" },
      { from: "b", to: "does-not-exist", relation: "optional", rawType: "optional" },
    ];
    const layout = layoutDependencyGraph(nodes, edges);
    expect(layout.nodes).toHaveLength(4);
    expect(layout.edges).toHaveLength(3);
    const byId = new Map(layout.nodes.map((item) => [item.node.id, item]));
    expect(byId.get("root")!.x).toBeLessThan(byId.get("a")!.x);
    expect(byId.get("a")!.x).toBe(byId.get("b")!.x);
    expect(byId.get("c")!.x).toBeGreaterThan(byId.get("a")!.x);
    expect(layout.width).toBeGreaterThan(0);
    expect(layout.height).toBeGreaterThan(0);
  });

  it("handles an empty graph", () => {
    expect(layoutDependencyGraph([], [])).toEqual({ nodes: [], edges: [], width: 0, height: 0 });
  });

  it("draws a path between node edges", () => {
    const layout = layoutDependencyGraph([node("root", 0), node("a", 1, "root")], [
      { from: "root", to: "a", relation: "required", rawType: "required" },
    ]);
    expect(edgePath(layout.edges[0].from, layout.edges[0].to)).toContain("M ");
  });
});
