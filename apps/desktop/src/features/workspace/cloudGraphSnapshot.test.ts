import { expect, test } from "bun:test";

import { cloudGraphSnapshotToWorkspaceEnvelope } from "./cloudGraphSnapshot";
import type { CloudGraphSnapshotResponse } from "./types";

test("adapts server Postgres graph snapshot into workspace envelope without local paths", () => {
  const snapshot: CloudGraphSnapshotResponse = {
    workspaceId: "ws_cloud",
    projection: "live",
    store: "postgres.graph",
    nodes: [
      {
        versionId: "nv_source",
        id: "source:src_1",
        kind: "source",
        label: "Cloud Source",
        scope: "workspace",
        aliases: ["Cloud Source"],
        evidenceIds: ["ev_src_1_root"],
        sourceIds: ["src_1"],
        confidence: 1,
        createdByEventId: "materialize:src_1",
        validFrom: 10,
      },
      {
        versionId: "nv_ev",
        id: "evidence:ev_src_1_root",
        kind: "evidence",
        label: "Server-side materialized evidence",
        scope: "workspace",
        aliases: [],
        evidenceIds: ["ev_src_1_root"],
        sourceIds: ["src_1"],
        confidence: 0.9,
        createdByEventId: "materialize:src_1",
        validFrom: 11,
      },
    ],
    relations: [
      {
        versionId: "rv_1",
        id: "source_evidence:src_1:ev_src_1_root",
        kind: "contains_evidence",
        sourceNodeId: "source:src_1",
        targetNodeId: "evidence:ev_src_1_root",
        label: "contains evidence",
        evidenceIds: ["ev_src_1_root"],
        confidence: 0.9,
        createdByEventId: "materialize:src_1",
        validFrom: 12,
      },
    ],
    claims: [],
  };

  const envelope = cloudGraphSnapshotToWorkspaceEnvelope(snapshot);

  expect(envelope.workspace_id).toBe("ws_cloud");
  expect(envelope.project?.summary.projectId).toBe("workspace:ws_cloud");
  expect(envelope.project?.summary.relationshipCount).toBe(1);
  expect(envelope.sources).toHaveLength(1);
  expect(envelope.sources[0].source_path).toBe("cloud://source/src_1");
  expect(envelope.project?.nodes.map((node) => node.id)).toEqual([
    "source:src_1",
    "evidence:ev_src_1_root",
  ]);
  expect(envelope.project?.detailsByNodeId["source:src_1"]?.source?.sourcePath).toBe(
    "cloud://source/src_1",
  );
  expect(envelope.project?.detailsByNodeId["evidence:ev_src_1_root"]?.evidence[0].id).toBe(
    "ev_src_1_root",
  );
});
