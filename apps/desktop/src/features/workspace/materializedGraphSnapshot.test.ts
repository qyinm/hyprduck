import { expect, test } from "bun:test";

import { materializedGraphSnapshotToWorkspaceEnvelope } from "./materializedGraphSnapshot";
import type { MaterializedGraphSnapshot } from "./types";

test("keeps derived markdown as a source artifact instead of a graph node", () => {
  const snapshot: MaterializedGraphSnapshot = {
    snapshotId: "snapshot-1",
    sourceIngestId: "ingest-1",
    workspaceId: "default",
    sourceOfTruthPath: "events/brain_events.jsonl",
    latestReadableSnapshotPath: "state/latest-readable-snapshot.json",
    createdAt: 1,
    materializedAt: 2,
    materializedPaths: ["graph/nodes.json", "graph/edges.json", "wiki/index.md"],
    sourcePaths: [
      "/brain/default/sources/source-1/report.pdf",
      "/brain/default/artifacts/source-1/report.md",
    ],
    nodes: [
      {
        nodeId: "source-pdf",
        kind: "source",
        label: "report.pdf",
        aliases: [],
        evidenceIds: [],
        sourceIds: ["source-1"],
        confidence: 1,
        updatedAt: 2,
      },
      {
        nodeId: "source-md",
        kind: "source",
        label: "report.md",
        aliases: [],
        evidenceIds: [],
        sourceIds: ["source-1"],
        confidence: 1,
        updatedAt: 2,
      },
      {
        nodeId: "concept-a",
        kind: "concept",
        label: "Project Alpha",
        aliases: [],
        evidenceIds: [],
        sourceIds: ["source-1"],
        confidence: 0.8,
        updatedAt: 2,
      },
    ],
    edges: [
      {
        relationId: "edge-visible",
        kind: "source_of",
        sourceNodeId: "source-pdf",
        targetNodeId: "concept-a",
        label: "source of",
        evidenceIds: [],
        confidence: 1,
        updatedAt: 2,
      },
      {
        relationId: "edge-hidden",
        kind: "derived_from",
        sourceNodeId: "source-md",
        targetNodeId: "concept-a",
        label: "derived from",
        evidenceIds: [],
        confidence: 1,
        updatedAt: 2,
      },
    ],
    claims: [],
    memoryRefs: [],
    wikiPages: [],
  };

  const envelope = materializedGraphSnapshotToWorkspaceEnvelope(snapshot);

  expect(envelope.project.nodes.map((node) => node.label)).toEqual([
    "report.pdf",
    "Project Alpha",
  ]);
  expect(envelope.project.edges.map((edge) => edge.id)).toEqual(["edge-visible"]);
  expect(envelope.project.summary.documentCount).toBe(1);
  expect(envelope.sources).toHaveLength(1);
  expect(envelope.sources[0].source_path).toBe("/brain/default/sources/source-1/report.pdf");
  expect(envelope.sources[0].markdown_path).toBe(
    "/brain/default/artifacts/source-1/report.md",
  );
  expect(envelope.project.detailsByNodeId["source-pdf"]?.source?.sourcePath).toBe(
    "/brain/default/sources/source-1/report.pdf",
  );
  expect(envelope.project.detailsByNodeId["source-pdf"]?.source?.markdownPath).toBe(
    "/brain/default/artifacts/source-1/report.md",
  );
});

test("shows derived_from source edges when both endpoints are visible", () => {
  const snapshot: MaterializedGraphSnapshot = {
    snapshotId: "snapshot-1",
    sourceIngestId: "ingest-1",
    workspaceId: "default",
    sourceOfTruthPath: "events/brain_events.jsonl",
    latestReadableSnapshotPath: "state/latest-readable-snapshot.json",
    createdAt: 1,
    materializedAt: 2,
    materializedPaths: ["graph/nodes.json", "graph/edges.json"],
    sourcePaths: ["/brain/default/sources/source-1/report.pdf"],
    nodes: [
      {
        nodeId: "source-pdf",
        kind: "source",
        label: "report.pdf",
        aliases: [],
        evidenceIds: [],
        sourceIds: ["source-1"],
        confidence: 1,
        updatedAt: 2,
      },
      {
        nodeId: "concept-a",
        kind: "concept",
        label: "Project Alpha",
        aliases: [],
        evidenceIds: [],
        sourceIds: ["source-1"],
        confidence: 0.8,
        updatedAt: 2,
      },
    ],
    edges: [
      {
        relationId: "edge-derived-visible-endpoints",
        kind: "derived_from",
        sourceNodeId: "source-pdf",
        targetNodeId: "concept-a",
        label: "derived from",
        evidenceIds: [],
        confidence: 1,
        updatedAt: 2,
      },
    ],
    claims: [],
    memoryRefs: [],
    wikiPages: [],
  };

  const envelope = materializedGraphSnapshotToWorkspaceEnvelope(snapshot);

  expect(envelope.project.nodes.map((node) => node.id)).toEqual([
    "source-pdf",
    "concept-a",
  ]);
  expect(envelope.project.edges).toEqual([
    expect.objectContaining({
      id: "edge-derived-visible-endpoints",
      kind: "source_document",
    }),
  ]);
});

test("matches source artifact paths by segment instead of substring", () => {
  const snapshot: MaterializedGraphSnapshot = {
    snapshotId: "snapshot-1",
    sourceIngestId: "ingest-1",
    workspaceId: "default",
    sourceOfTruthPath: "events/brain_events.jsonl",
    latestReadableSnapshotPath: "state/latest-readable-snapshot.json",
    createdAt: 1,
    materializedAt: 2,
    materializedPaths: ["graph/nodes.json", "graph/edges.json", "wiki/index.md"],
    sourcePaths: [
      "/brain/default/sources/source-10/other.pdf",
      "/brain/default/artifacts/source-10/other.md",
      "/brain/default/sources/source-1/report.pdf",
      "/brain/default/artifacts/source-1/report.md",
    ],
    nodes: [
      {
        nodeId: "source-pdf",
        kind: "source",
        label: "report.pdf",
        aliases: [],
        evidenceIds: ["ev-1"],
        sourceIds: ["source-1"],
        confidence: 1,
        updatedAt: 2,
      },
      {
        nodeId: "source-md",
        kind: "source",
        label: "report.md",
        aliases: [],
        evidenceIds: [],
        sourceIds: ["source-1"],
        confidence: 1,
        updatedAt: 2,
      },
      {
        nodeId: "other-pdf",
        kind: "source",
        label: "other.pdf",
        aliases: [],
        evidenceIds: [],
        sourceIds: ["source-10"],
        confidence: 1,
        updatedAt: 2,
      },
    ],
    edges: [],
    claims: [],
    memoryRefs: [],
    wikiPages: [],
  };

  const envelope = materializedGraphSnapshotToWorkspaceEnvelope(snapshot);

  expect(envelope.project.nodes.map((node) => node.id)).not.toContain("source-md");
  expect(envelope.project.detailsByNodeId["source-pdf"]?.source?.sourcePath).toBe(
    "/brain/default/sources/source-1/report.pdf",
  );
  expect(envelope.project.detailsByNodeId["source-pdf"]?.source?.markdownPath).toBe(
    "/brain/default/artifacts/source-1/report.md",
  );
  expect(envelope.project.detailsByNodeId["source-pdf"]?.evidence[0]?.sourcePath).toBe(
    "/brain/default/sources/source-1/report.pdf",
  );
  expect(envelope.sources.map((source) => source.source_id)).toEqual([
    "source-10",
    "source-1",
  ]);
});

test("matches source paths from exact sourceIds before falling back to source-* artifacts", () => {
  const snapshot: MaterializedGraphSnapshot = {
    snapshotId: "snapshot-1",
    sourceIngestId: "ingest-1",
    workspaceId: "default",
    sourceOfTruthPath: "events/brain_events.jsonl",
    latestReadableSnapshotPath: "state/latest-readable-snapshot.json",
    createdAt: 1,
    materializedAt: 2,
    materializedPaths: ["graph/nodes.json", "graph/edges.json", "wiki/index.md"],
    sourcePaths: [
      "/brain/default/sources/import-alpha/product-plan.pdf",
      "/brain/default/artifacts/import-alpha/product-plan.md",
      "/brain/default/sources/source-backup/old-plan.pdf",
    ],
    nodes: [
      {
        nodeId: "source-product-plan",
        kind: "source",
        label: "product-plan.pdf",
        aliases: [],
        evidenceIds: ["ev-plan"],
        sourceIds: ["import-alpha"],
        confidence: 1,
        updatedAt: 2,
      },
      {
        nodeId: "concept-plan",
        kind: "concept",
        label: "Product Plan",
        aliases: [],
        evidenceIds: ["ev-plan"],
        sourceIds: ["import-alpha"],
        confidence: 0.8,
        updatedAt: 2,
      },
    ],
    edges: [],
    claims: [],
    memoryRefs: [],
    wikiPages: [],
  };

  const envelope = materializedGraphSnapshotToWorkspaceEnvelope(snapshot);

  expect(envelope.sources.map((source) => source.source_id)).toEqual([
    "import-alpha",
    "source-backup",
  ]);
  expect(envelope.project.detailsByNodeId["source-product-plan"]?.source?.sourceId).toBe(
    "import-alpha",
  );
  expect(envelope.project.detailsByNodeId["source-product-plan"]?.source?.sourcePath).toBe(
    "/brain/default/sources/import-alpha/product-plan.pdf",
  );
  expect(envelope.project.detailsByNodeId["source-product-plan"]?.source?.markdownPath).toBe(
    "/brain/default/artifacts/import-alpha/product-plan.md",
  );
  expect(envelope.project.detailsByNodeId["concept-plan"]?.evidence[0]?.sourceId).toBe(
    "import-alpha",
  );
  expect(envelope.project.detailsByNodeId["concept-plan"]?.evidence[0]?.sourcePath).toBe(
    "/brain/default/sources/import-alpha/product-plan.pdf",
  );
});

test("hydrates wiki page evidence with the page body and source refs available in the snapshot", () => {
  const snapshot: MaterializedGraphSnapshot = {
    snapshotId: "snapshot-1",
    sourceIngestId: "ingest-1",
    workspaceId: "default",
    sourceOfTruthPath: "events/brain_events.jsonl",
    latestReadableSnapshotPath: "state/latest-readable-snapshot.json",
    createdAt: 1,
    materializedAt: 2,
    materializedPaths: ["graph/nodes.json", "graph/edges.json", "wiki/index.md"],
    sourcePaths: [
      "/brain/default/sources/import-alpha/product-plan.pdf",
      "/brain/default/artifacts/import-alpha/product-plan.md",
    ],
    nodes: [
      {
        nodeId: "concept-plan",
        kind: "concept",
        label: "Product Plan",
        aliases: [],
        evidenceIds: ["ev-plan"],
        sourceIds: ["import-alpha"],
        confidence: 0.8,
        updatedAt: 2,
      },
    ],
    edges: [],
    claims: [],
    memoryRefs: [],
    wikiPages: [
      {
        pageId: "page-plan",
        workspaceId: "default",
        path: "wiki/product-plan.md",
        title: "Product Plan",
        body: "The product plan prioritizes local parsing and grounded evidence.",
        nodeRefs: ["concept-plan"],
        sourceRefs: ["import-alpha"],
        evidenceRefs: ["ev-plan"],
        updatedAt: 2,
      },
    ],
  };

  const envelope = materializedGraphSnapshotToWorkspaceEnvelope(snapshot);
  const evidence = envelope.project.detailsByNodeId["concept-plan"]?.evidence[0];

  expect(evidence?.snippet).toBe(
    "The product plan prioritizes local parsing and grounded evidence.",
  );
  expect(evidence?.sourceId).toBe("import-alpha");
  expect(evidence?.sourcePath).toBe("/brain/default/sources/import-alpha/product-plan.pdf");
  expect(evidence?.markdownPath).toBe(
    "/brain/default/artifacts/import-alpha/product-plan.md",
  );
  expect(evidence?.provenance).toBe("wiki/product-plan.md");
});

test("hydrates delete correction actions for materialized graph nodes", () => {
  const snapshot: MaterializedGraphSnapshot = {
    snapshotId: "snapshot-1",
    sourceIngestId: "ingest-1",
    workspaceId: "default",
    sourceOfTruthPath: "events/brain_events.jsonl",
    latestReadableSnapshotPath: "state/latest-readable-snapshot.json",
    createdAt: 1,
    materializedAt: 2,
    materializedPaths: ["graph/nodes.json", "graph/edges.json", "wiki/index.md"],
    sourcePaths: ["/brain/default/sources/source-1/report.pdf"],
    nodes: [
      {
        nodeId: "source:source-1",
        kind: "source",
        label: "report.pdf",
        aliases: [],
        evidenceIds: ["ev-source"],
        sourceIds: ["source-1"],
        confidence: 1,
        updatedAt: 2,
      },
      {
        nodeId: "concept-a",
        kind: "concept",
        label: "Project Alpha",
        aliases: [],
        evidenceIds: ["ev-concept"],
        sourceIds: ["source-1"],
        confidence: 0.8,
        updatedAt: 2,
      },
    ],
    edges: [],
    claims: [],
    memoryRefs: [],
    wikiPages: [],
  };

  const envelope = materializedGraphSnapshotToWorkspaceEnvelope(snapshot);

  expect(envelope.project.detailsByNodeId["source:source-1"]?.actions).toEqual([
    { kind: "delete", label: "Delete", disabledReason: null },
  ]);
  expect(envelope.project.detailsByNodeId["concept-a"]?.actions).toContainEqual({
    kind: "delete",
    label: "Delete",
    disabledReason: null,
  });
});
