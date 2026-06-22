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
      id: "source:source-1",
      label: "Source PDF",
      kind: "source",
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
      sourceNodeId: "source:source-1",
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
    expect(Number.isFinite(graph.getNodeAttribute("source:source-1", "x"))).toBe(true);
    expect(Number.isFinite(graph.getNodeAttribute("source:source-1", "y"))).toBe(true);
    expect(distanceBetween(graph, "source:source-1", "concept")).toBeLessThan(0.35);
    expect(graph.getNodeAttribute("source:source-1", "nodeKind")).toBe("source");
    expect(graph.getNodeAttribute("source:source-1", "shortLabel")).toBe("Source PDF");
    expect(graph.getNodeAttribute("source:source-1", "clusterId")).toBe(
      graph.getNodeAttribute("concept", "clusterId"),
    );
    expect(graph.getNodeAttribute("source:source-1", "size")).toBeGreaterThan(
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
            sourceNodeId: "source:source-1",
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

  test("skips duplicate graph keys instead of throwing away the graph", () => {
    const graph = buildSigmaGraph(
      {
        ...project,
        nodes: [...project.nodes, { ...project.nodes[1], label: "Duplicate Concept" }],
        edges: [
          ...project.edges,
          {
            ...project.edges[0],
            label: "Duplicate edge",
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
    expect(graph.getNodeAttribute("concept", "label")).toBe("Concept");
    expect(graph.getEdgeAttribute("edge-document-concept", "label")).toBe(
      "Ingested from source",
    );
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

    expect(Number.isFinite(graph.getNodeAttribute("source:source-1", "x"))).toBe(true);
    expect(Number.isFinite(graph.getNodeAttribute("source:source-1", "y"))).toBe(true);
    expect(Number.isFinite(graph.getNodeAttribute("concept", "x"))).toBe(true);
    expect(Number.isFinite(graph.getNodeAttribute("concept", "y"))).toBe(true);
  });

  test("clusters connected components while keeping unrelated nodes separated", () => {
    const graph = buildSigmaGraph(
      {
        ...project,
        nodes: [
          ...project.nodes,
          {
            id: "isolated-a",
            label: "Isolated A",
            kind: "concept",
            confidence: null,
            relatedCount: 0,
            evidenceCount: 0,
            position: { x: 50, y: 50 },
          },
          {
            id: "isolated-b",
            label: "Isolated B",
            kind: "concept",
            confidence: null,
            relatedCount: 0,
            evidenceCount: 0,
            position: { x: 51, y: 51 },
          },
        ],
      },
      {
        selectedNodeId: null,
        selectedEdgeId: null,
      },
    );

    const connectedDistance = distanceBetween(graph, "source:source-1", "concept");
    const unrelatedDistance = distanceBetween(graph, "source:source-1", "isolated-a");

    expect(connectedDistance).toBeLessThan(unrelatedDistance);
    expect(distanceBetween(graph, "isolated-a", "isolated-b")).toBeGreaterThan(0.3);
  });

  test("separates dense nodes so they do not stack on top of each other", () => {
    const denseNodes = Array.from({ length: 14 }, (_, index) => ({
      id: `dense-${index}`,
      label: `Dense ${index}`,
      kind: "concept" as const,
      confidence: null,
      relatedCount: 2,
      evidenceCount: 1,
      position: { x: 50, y: 50 },
    }));
    const denseEdges = denseNodes.slice(1).map((node, index) => ({
      id: `dense-edge-${index}`,
      sourceNodeId: "dense-0",
      targetNodeId: node.id,
      kind: "related_to",
      label: "Related",
      confidence: null,
      evidenceCount: 1,
    }));
    const graph = buildSigmaGraph(
      {
        ...project,
        nodes: denseNodes,
        edges: denseEdges,
      },
      {
        selectedNodeId: null,
        selectedEdgeId: null,
      },
    );

    for (let sourceIndex = 0; sourceIndex < denseNodes.length; sourceIndex += 1) {
      for (let targetIndex = sourceIndex + 1; targetIndex < denseNodes.length; targetIndex += 1) {
        expect(
          distanceBetween(graph, denseNodes[sourceIndex].id, denseNodes[targetIndex].id),
        ).toBeGreaterThan(0.12);
      }
    }
  });

  test("keeps source-centered clusters separated even when concepts have cross-links", () => {
    const clusterProject: WorkspaceProject = {
      ...project,
      nodes: [
        {
          ...project.nodes[0],
          id: "source:a",
          label: "Source A",
        },
        {
          ...project.nodes[1],
          id: "concept:a",
          label: "Concept A",
        },
        {
          ...project.nodes[0],
          id: "source:b",
          label: "Source B",
        },
        {
          ...project.nodes[1],
          id: "concept:b",
          label: "Concept B",
        },
      ],
      edges: [
        {
          id: "source-a-concept-a",
          sourceNodeId: "source:a",
          targetNodeId: "concept:a",
          kind: "source_document",
          label: "Ingested from source",
          confidence: 0.8,
          evidenceCount: 1,
        },
        {
          id: "source-b-concept-b",
          sourceNodeId: "source:b",
          targetNodeId: "concept:b",
          kind: "source_document",
          label: "Ingested from source",
          confidence: 0.8,
          evidenceCount: 1,
        },
        {
          id: "cross-concept-link",
          sourceNodeId: "concept:a",
          targetNodeId: "concept:b",
          kind: "related_to",
          label: "Related",
          confidence: 0.4,
          evidenceCount: 1,
        },
      ],
    };
    const graph = buildSigmaGraph(clusterProject, {
      selectedNodeId: null,
      selectedEdgeId: null,
    });

    expect(distanceBetween(graph, "source:a", "concept:a")).toBeLessThan(0.45);
    expect(distanceBetween(graph, "source:b", "concept:b")).toBeLessThan(0.45);
    expect(distanceBetween(graph, "source:a", "source:b")).toBeGreaterThan(0.65);
    expect(graph.getNodeAttribute("source:a", "clusterId")).not.toBe(
      graph.getNodeAttribute("source:b", "clusterId"),
    );
  });

  test("shortens long document labels while preserving the file extension", () => {
    const graph = buildSigmaGraph(
      {
        ...project,
        nodes: [
          {
            ...project.nodes[0],
            label: "2026년_상반기_고객_인터뷰_통합_분석_보고서.pdf",
          },
        ],
        edges: [],
      },
      {
        selectedNodeId: null,
        selectedEdgeId: null,
      },
    );

    const label = graph.getNodeAttribute("source:source-1", "shortLabel");
    expect(label).toContain("…");
    expect(label.endsWith(".pdf")).toBe(true);
  });

  test("local graph scope keeps only the selected node neighborhood", () => {
    const graph = buildSigmaGraph(
      {
        ...project,
        nodes: [
          ...project.nodes,
          {
            id: "neighbor",
            label: "Neighbor",
            kind: "concept",
            confidence: null,
            relatedCount: 1,
            evidenceCount: 1,
            position: { x: 50, y: 50 },
          },
          {
            id: "remote",
            label: "Remote",
            kind: "concept",
            confidence: null,
            relatedCount: 0,
            evidenceCount: 0,
            position: { x: 50, y: 50 },
          },
        ],
        edges: [
          ...project.edges,
          {
            id: "edge-concept-neighbor",
            sourceNodeId: "concept",
            targetNodeId: "neighbor",
            kind: "related_to",
            label: "Related",
            confidence: null,
            evidenceCount: 1,
          },
        ],
      },
      {
        selectedNodeId: "concept",
        selectedEdgeId: null,
      },
      {
        mode: "local",
        centerNodeId: "concept",
      },
    );

    expect(graph.hasNode("concept")).toBe(true);
    expect(graph.hasNode("source:source-1")).toBe(true);
    expect(graph.hasNode("neighbor")).toBe(true);
    expect(graph.hasNode("remote")).toBe(false);
    expect(graph.size).toBe(2);
  });
});

function distanceBetween(
  graph: ReturnType<typeof buildSigmaGraph>,
  source: string,
  target: string,
): number {
  const sourceAttrs = graph.getNodeAttributes(source);
  const targetAttrs = graph.getNodeAttributes(target);
  return Math.hypot(sourceAttrs.x - targetAttrs.x, sourceAttrs.y - targetAttrs.y);
}
