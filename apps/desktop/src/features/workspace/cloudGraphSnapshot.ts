import { materializedGraphSnapshotToWorkspaceEnvelope } from "./materializedGraphSnapshot";
import type {
  CloudGraphSnapshotResponse,
  MaterializedGraphRelationKind,
  MaterializedGraphSnapshot,
  WorkspaceProjectEnvelope,
} from "./types";

export function cloudGraphSnapshotToWorkspaceEnvelope(
  snapshot: CloudGraphSnapshotResponse,
): WorkspaceProjectEnvelope {
  return materializedGraphSnapshotToWorkspaceEnvelope(
    cloudGraphSnapshotToMaterializedSnapshot(snapshot),
  );
}

export function cloudGraphSnapshotToMaterializedSnapshot(
  snapshot: CloudGraphSnapshotResponse,
): MaterializedGraphSnapshot {
  const sourceIds = Array.from(
    new Set(snapshot.nodes.flatMap((node) => node.sourceIds)),
  ).sort();
  const materializedAt = Math.max(
    0,
    ...snapshot.nodes.map((node) => node.validFrom),
    ...snapshot.relations.map((relation) => relation.validFrom),
    ...snapshot.claims.map((claim) => claim.validFrom),
  );

  return {
    snapshotId: `${snapshot.workspaceId}:${snapshot.projection}:${snapshot.store}`,
    sourceIngestId: "etyma-server-cloud",
    workspaceId: snapshot.workspaceId,
    sourceOfTruthPath: snapshot.store,
    latestReadableSnapshotPath: "/v1/graph/snapshot",
    createdAt: materializedAt,
    materializedAt,
    materializedPaths: ["/v1/graph/snapshot"],
    sourcePaths: sourceIds.map((sourceId) => cloudSourceHandle(sourceId)),
    sources: sourceIds.map((sourceId) => ({
      sourceId,
      workspaceId: snapshot.workspaceId,
      originalPath: cloudSourceHandle(sourceId),
      sourcePath: cloudSourceHandle(sourceId),
      markdownPath: cloudSourceHandle(sourceId),
      format: "cloud",
      status: "ingested",
      pageCount: 0,
      successCount: 0,
      failedCount: 0,
      description: "Cloud source from the server graph snapshot.",
      userContext: "",
      ingestInstruction: "",
      updatedAt: materializedAt,
    })),
    nodes: snapshot.nodes.map((node) => ({
      nodeId: node.id,
      kind: materializedNodeKind(node.kind),
      label: node.label,
      aliases: node.aliases,
      evidenceIds: node.evidenceIds,
      sourceIds: node.sourceIds,
      confidence: node.confidence,
      updatedAt: node.validFrom,
      validFrom: node.validFrom,
      validTo: null,
      supersededBy: null,
    })),
    edges: snapshot.relations.map((relation) => ({
      relationId: relation.id,
      kind: materializedRelationKind(relation.kind),
      sourceNodeId: relation.sourceNodeId,
      targetNodeId: relation.targetNodeId,
      label: relation.label,
      evidenceIds: relation.evidenceIds,
      confidence: relation.confidence,
      updatedAt: relation.validFrom,
      validFrom: relation.validFrom,
      validTo: null,
      supersededBy: null,
    })),
    claims: snapshot.claims,
    memoryRefs: [],
    wikiPages: [],
  };
}

function cloudSourceHandle(sourceId: string) {
  return `cloud://source/${sourceId}`;
}

function materializedNodeKind(kind: string) {
  if (kind === "source" || kind === "evidence") {
    return kind === "evidence" ? "concept" : "source";
  }
  return "concept";
}

function materializedRelationKind(kind: string): MaterializedGraphRelationKind {
  return kind === "contains_evidence" || kind === "source_of" || kind === "derived_from"
    ? "source_of"
    : "related_to";
}
