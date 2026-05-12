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
