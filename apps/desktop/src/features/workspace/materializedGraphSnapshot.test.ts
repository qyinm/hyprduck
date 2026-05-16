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

test("hides derived_from plumbing edges even when both endpoints are visible", () => {
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
  expect(envelope.project.edges).toEqual([]);
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
