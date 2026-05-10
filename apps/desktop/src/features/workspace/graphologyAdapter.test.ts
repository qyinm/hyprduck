import { describe, expect, test } from "bun:test";

import { buildSigmaGraph } from "./graphologyAdapter";
import type { WorkspaceProject } from "./types";

const project: WorkspaceProject = {
  summary: {
    projectId: "test",
    title: "Test",
    status: "preview",
    stale: false,
    summary: "",
    documentCount: 1,
    nodeCount: 2,
    relationshipCount: 1,
    evidenceCount: 3,
  },
  nodes: [
    {
      id: "document",
      label: "Source PDF",
      kind: "document",
      confidence: 0.8,
      relatedCount: 1,
      evidenceCount: 2,
      position: { x: 48, y: 16 },
    },
    {
      id: "concept",
      label: "Concept",
      kind: "concept",
      confidence: 0.7,
      relatedCount: 1,
      evidenceCount: 1,
      position: { x: 54, y: 64 },
    },
  ],
  edges: [
    {
      id: "edge-document-concept",
      sourceNodeId: "document",
      targetNodeId: "concept",
      kind: "source_document",
      label: "Ingested from source",
      confidence: 0.6,
      evidenceCount: 1,
    },
  ],
  detailsByNodeId: {},
  edgeDetailsById: {},
  answerByNodeId: {},
};

describe("buildSigmaGraph", () => {
  test("converts workspace nodes and edges into a selectable Graphology graph", () => {
    const graph = buildSigmaGraph(project, {
      selectedNodeId: "concept",
      selectedEdgeId: null,
    });

    expect(graph.order).toBe(2);
    expect(graph.size).toBe(1);
    expect(graph.getNodeAttribute("document", "x")).toBeCloseTo(-0.04);
    expect(graph.getNodeAttribute("document", "y")).toBeCloseTo(0.68);
    expect(graph.getNodeAttribute("document", "nodeKind")).toBe("document");
    expect(graph.getNodeAttribute("document", "size")).toBeGreaterThan(
      graph.getNodeAttribute("concept", "size"),
    );
    expect(graph.getNodeAttribute("concept", "selected")).toBe(true);
    expect(graph.getNodeAttribute("concept", "color")).toBe("#111111");
    expect(graph.getEdgeAttribute("edge-document-concept", "edgeKind")).toBe(
      "source_document",
    );
    expect(graph.getEdgeAttribute("edge-document-concept", "hidden")).toBe(false);
  });

  test("hides invalid edges instead of throwing away the graph", () => {
    const graph = buildSigmaGraph(
      {
        ...project,
        edges: [
          ...project.edges,
          {
            id: "missing-target",
            sourceNodeId: "document",
            targetNodeId: "missing",
            kind: "related_to",
            label: "Missing",
            confidence: null,
            evidenceCount: 0,
          },
        ],
      },
      {
        selectedNodeId: null,
        selectedEdgeId: "edge-document-concept",
      },
    );

    expect(graph.order).toBe(2);
    expect(graph.size).toBe(1);
    expect(graph.getEdgeAttribute("edge-document-concept", "selected")).toBe(true);
  });

  test("skips self-loop edges instead of throwing away the graph", () => {
    const graph = buildSigmaGraph(
      {
        ...project,
        edges: [
          ...project.edges,
          {
            id: "self-loop",
            sourceNodeId: "concept",
            targetNodeId: "concept",
            kind: "related_to",
            label: "Self loop",
            confidence: null,
            evidenceCount: 0,
          },
        ],
      },
      {
        selectedNodeId: null,
        selectedEdgeId: null,
      },
    );

    expect(graph.order).toBe(2);
    expect(graph.size).toBe(1);
    expect(graph.hasEdge("self-loop")).toBe(false);
  });

  test("assigns fallback coordinates when persisted nodes do not have valid positions", () => {
    const graph = buildSigmaGraph(
      {
        ...project,
        nodes: [
          {
            ...project.nodes[0],
            position: undefined,
          },
          {
            ...project.nodes[1],
            position: { x: Number.NaN, y: Number.POSITIVE_INFINITY },
          },
        ],
      } as unknown as WorkspaceProject,
      {
        selectedNodeId: null,
        selectedEdgeId: null,
      },
    );

    expect(Number.isFinite(graph.getNodeAttribute("document", "x"))).toBe(true);
    expect(Number.isFinite(graph.getNodeAttribute("document", "y"))).toBe(true);
    expect(Number.isFinite(graph.getNodeAttribute("concept", "x"))).toBe(true);
    expect(Number.isFinite(graph.getNodeAttribute("concept", "y"))).toBe(true);
  });
});
