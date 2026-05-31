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

test("caps materialized workspace projection and reports hidden counts", () => {
  const conceptCount = 75;
  const relatedCount = 120;
  const nodes: MaterializedGraphSnapshot["nodes"] = [
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
  ];
  const edges: MaterializedGraphSnapshot["edges"] = [];

  for (let index = 0; index < conceptCount; index += 1) {
    nodes.push({
      nodeId: `concept-${index.toString().padStart(3, "0")}`,
      kind: "concept",
      label: `Projection Concept ${index.toString().padStart(3, "0")}`,
      aliases: [],
      evidenceIds: Array.from({ length: Math.min(5, conceptCount - index) }, (_, evidenceIndex) =>
        `ev-${index}-${evidenceIndex}`,
      ),
      sourceIds: ["source-1"],
      confidence: 0.9,
      updatedAt: 2,
    });
    edges.push({
      relationId: `edge-source-${index.toString().padStart(3, "0")}`,
      kind: "source_of",
      sourceNodeId: "source-pdf",
      targetNodeId: `concept-${index.toString().padStart(3, "0")}`,
      label: "source of",
      evidenceIds: [`ev-${index}-0`],
      confidence: 0.8,
      updatedAt: 2,
    });
  }
  for (let index = 0; index < relatedCount; index += 1) {
    edges.push({
      relationId: `edge-related-${index.toString().padStart(3, "0")}`,
      kind: "related_to",
      sourceNodeId: `concept-${(index % conceptCount).toString().padStart(3, "0")}`,
      targetNodeId: `concept-${((index + 1) % conceptCount).toString().padStart(3, "0")}`,
      label: "related to",
      evidenceIds: [`ev-${index % conceptCount}-0`],
      confidence: 0.7,
      updatedAt: 2,
    });
  }
  const snapshot: MaterializedGraphSnapshot = {
    snapshotId: "snapshot-large",
    sourceIngestId: "ingest-large",
    workspaceId: "default",
    sourceOfTruthPath: "events/brain_events.jsonl",
    latestReadableSnapshotPath: "state/latest-readable-snapshot.json",
    createdAt: 1,
    materializedAt: 2,
    materializedPaths: ["graph/nodes.json", "graph/edges.json"],
    sourcePaths: ["/brain/default/sources/source-1/report.pdf"],
    graphMaterializationReports: [
      {
        sourceId: "source-1",
        status: "linked",
        stage: "linked",
        progress: 1,
        sourceGraphMaterialized: true,
        workspaceLinkingMaterialized: true,
        rawSourceGraphNodeCount: 151,
        rawSourceGraphRelationCount: 150,
        canonicalSourceGraphNodeCount: 76,
        canonicalSourceGraphRelationCount: 120,
        prunedSourceGraphNodeCount: 75,
        prunedSourceGraphRelationCount: 30,
        compactionStatus: "compacted",
      },
    ],
    nodes,
    edges,
    claims: [],
    memoryRefs: [],
    wikiPages: [],
  };

  const envelope = materializedGraphSnapshotToWorkspaceEnvelope(snapshot);
  const visibleConcepts = envelope.project.nodes.filter((node) => node.kind === "concept");
  const visibleNodeIds = new Set(envelope.project.nodes.map((node) => node.id));

  expect(visibleConcepts.length).toBeLessThanOrEqual(60);
  expect(envelope.project.nodes.filter((node) => node.kind === "source")).toHaveLength(1);
  expect(envelope.project.edges.length).toBeLessThanOrEqual(90);
  expect(envelope.project.summary.hiddenConceptCount).toBe(15);
  expect(envelope.project.summary.compactionSummary).toBe(
    "75 canonical concepts -> 60 visible concepts",
  );
  expect(envelope.project.summary.graphMaterializationSummary).toBe(
    "151 raw nodes -> 76 canonical nodes -> 61 visible nodes · 150 raw links -> 120 canonical links -> 90 visible links · 1/1 sources complete",
  );
  expect(envelope.project.summary.hiddenRelationCount).toBe(
    conceptCount + relatedCount - envelope.project.edges.length,
  );
  expect(
    envelope.project.edges.every(
      (edge) => visibleNodeIds.has(edge.sourceNodeId) && visibleNodeIds.has(edge.targetNodeId),
    ),
  ).toBe(true);
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

test("keeps generated wiki pages out of the default graph canvas", () => {
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
    sources: [
      {
        sourceId: "source-1",
        workspaceId: "default",
        originalPath: "source-fixture.pdf",
        sourcePath: "/brain/default/sources/source-1/report.pdf",
        markdownPath: "/brain/default/artifacts/source-1/report.md",
        format: "pdf",
        status: "ingested",
        pageCount: 4,
        successCount: 3,
        failedCount: 1,
        description: "Imported source fixture",
        userContext: "",
        ingestInstruction: "",
        updatedAt: 2,
      },
    ],
    nodes: [
      {
        nodeId: "source:source-1",
        kind: "source",
        label: "report.pdf",
        aliases: [],
        evidenceIds: [],
        sourceIds: ["source-1"],
        confidence: 1,
        updatedAt: 2,
      },
      {
        nodeId: "concept-alpha",
        kind: "concept",
        label: "Project Alpha",
        aliases: [],
        evidenceIds: ["ev-alpha"],
        sourceIds: ["source-1"],
        confidence: 0.8,
        updatedAt: 2,
      },
      {
        nodeId: "claim-alpha",
        kind: "claim",
        label: "Alpha claim synthesized from evidence.",
        aliases: [],
        evidenceIds: ["ev-alpha"],
        sourceIds: ["source-1"],
        confidence: null,
        updatedAt: 2,
      },
      {
        nodeId: "wiki-overview",
        kind: "wiki_page",
        label: "Workspace Overview",
        aliases: ["wiki/overview.md"],
        evidenceIds: ["ev-alpha"],
        sourceIds: ["source-1"],
        confidence: null,
        updatedAt: 2,
      },
      {
        nodeId: "wiki-log",
        kind: "wiki_page",
        label: "Brain Log",
        aliases: ["wiki/log.md"],
        evidenceIds: [],
        sourceIds: [],
        confidence: null,
        updatedAt: 2,
      },
      {
        nodeId: "wiki-index",
        kind: "wiki_page",
        label: "Brain Index",
        aliases: ["wiki/index.md"],
        evidenceIds: [],
        sourceIds: [],
        confidence: null,
        updatedAt: 2,
      },
      {
        nodeId: "wiki-source-source-1",
        kind: "wiki_page",
        label: "source-1",
        aliases: ["wiki/sources/source-1.md"],
        evidenceIds: [],
        sourceIds: ["source-1"],
        confidence: null,
        updatedAt: 2,
      },
      {
        nodeId: "wiki-topic-concept-alpha",
        kind: "wiki_page",
        label: "Project Alpha",
        aliases: ["wiki/topics/concept-alpha.md"],
        evidenceIds: ["ev-alpha"],
        sourceIds: ["source-1"],
        confidence: null,
        updatedAt: 2,
      },
    ],
    edges: [
      {
        relationId: "edge-source-alpha",
        kind: "source_of",
        sourceNodeId: "source:source-1",
        targetNodeId: "concept-alpha",
        label: "source of",
        evidenceIds: ["ev-alpha"],
        confidence: 1,
        updatedAt: 2,
      },
    ],
    claims: [],
    memoryRefs: [],
    wikiPages: [
      {
        pageId: "wiki-topic-concept-alpha",
        workspaceId: "default",
        path: "wiki/topics/concept-alpha.md",
        title: "Project Alpha",
        body: "Alpha evidence remains available from the generated wiki topic.",
        nodeRefs: ["concept-alpha"],
        sourceRefs: ["source-1"],
        evidenceRefs: ["ev-alpha"],
        updatedAt: 2,
      },
    ],
  };

  const envelope = materializedGraphSnapshotToWorkspaceEnvelope(snapshot);

  expect(envelope.project.nodes.map((node) => node.label)).toEqual([
    "report.pdf",
    "Project Alpha",
  ]);
  expect(envelope.project.nodes.some((node) => node.id.startsWith("wiki-"))).toBe(false);
  expect(envelope.project.nodes.some((node) => node.id.startsWith("claim-"))).toBe(false);
  expect(envelope.project.edges.map((edge) => edge.id)).toEqual(["edge-source-alpha"]);
  expect(envelope.project.detailsByNodeId["concept-alpha"]?.evidence[0]?.snippet).toBe(
    "Alpha evidence remains available from the generated wiki topic.",
  );
  expect(envelope.project.detailsByNodeId["source:source-1"]?.source?.pageCount).toBe(4);
  expect(envelope.project.detailsByNodeId["source:source-1"]?.source?.successCount).toBe(3);
  expect(envelope.project.detailsByNodeId["source:source-1"]?.source?.failedCount).toBe(1);
  expect(envelope.sources[0].page_count).toBe(4);
  expect(envelope.sources[0].success_count).toBe(3);
  expect(envelope.sources[0].failed_count).toBe(1);
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
