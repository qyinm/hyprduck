import { buildWorkspacePreview } from "@/features/workspace/buildWorkspacePreview";
import type {
  MaterializedGraphSnapshot,
  WorkspaceProjectEnvelope,
} from "@/features/workspace/types";
import type { SourceDetailRequest, UiSnapshot } from "@/appTypes";

import { WEB_MOCK_MARKDOWN, WEB_MOCK_NOW_SECONDS } from "../fixtures";
import { webMockSnapshot } from "../state";

export function getWebWorkspaceFromSnapshot(
  snapshot: UiSnapshot = webMockSnapshot,
): WorkspaceProjectEnvelope {
  if (!snapshot.lastResult) {
    return { project: null, workspace_id: "web-preview", sources: [] };
  }
  const project = buildWorkspacePreview(snapshot.lastResult, Boolean(snapshot.activeJob));
  return {
    project,
    workspace_id: "web-preview",
    sources: project
      ? [
          {
            workspace_id: "web-preview",
            source_id: "preview",
            original_path: snapshot.lastResult.savedOutputPath ?? "web-preview.md",
            source_path: snapshot.lastResult.savedOutputPath ?? "web-preview.md",
            markdown_path: snapshot.lastResult.savedOutputPath ?? "web-preview.md",
            format: "markdown",
            status: snapshot.activeJob ? "ingesting" : "ingested",
            page_count: snapshot.lastResult.successCount + snapshot.lastResult.failedCount,
            success_count: snapshot.lastResult.successCount,
            failed_count: snapshot.lastResult.failedCount,
            description: "",
            user_context: "",
            ingest_instruction: "",
            updated_at: 0,
          },
        ]
      : [],
  };
}

export function createWebMaterializedGraphSnapshot(): MaterializedGraphSnapshot {
  return {
    snapshotId: "snapshot-web-preview",
    sourceIngestId: "web-preview",
    workspaceId: "web-preview",
    sourceOfTruthPath: "events/brain_events.jsonl",
    latestReadableSnapshotPath: "state/latest-readable-snapshot.json",
    createdAt: WEB_MOCK_NOW_SECONDS,
    materializedAt: WEB_MOCK_NOW_SECONDS,
    materializedPaths: [
      "graph/nodes.json",
      "graph/edges.json",
      "wiki/index.md",
      "events/brain_events.jsonl",
    ],
    sourcePaths: [webMockSnapshot.lastResult?.savedOutputPath ?? "web-preview.md"],
    nodes: [
      {
        nodeId: "source:preview",
        kind: "source",
        label: "Web preview source",
        aliases: ["Latest import"],
        evidenceIds: ["ev-page-1"],
        sourceIds: ["preview"],
        confidence: 0.72,
        updatedAt: WEB_MOCK_NOW_SECONDS,
      },
      {
        nodeId: "concept-agent-ready-knowledge",
        kind: "concept",
        label: "Agent-ready knowledge",
        aliases: ["Materialized graph"],
        evidenceIds: ["ev-page-1"],
        sourceIds: ["preview"],
        confidence: 0.76,
        updatedAt: WEB_MOCK_NOW_SECONDS,
      },
    ],
    edges: [
      {
        relationId: "edge-preview-agent-ready-knowledge",
        kind: "derived_from",
        sourceNodeId: "source:preview",
        targetNodeId: "concept-agent-ready-knowledge",
        label: "Derived from source",
        evidenceIds: ["ev-page-1"],
        confidence: 0.74,
        updatedAt: WEB_MOCK_NOW_SECONDS,
      },
    ],
    claims: [],
    memoryRefs: [],
    wikiPages: [
      {
        pageId: "wiki-index",
        workspaceId: "web-preview",
        path: "wiki/index.md",
        title: "Workspace index",
        body: WEB_MOCK_MARKDOWN,
        nodeRefs: ["source:preview", "concept-agent-ready-knowledge"],
        sourceRefs: ["preview"],
        evidenceRefs: ["ev-page-1"],
        updatedAt: WEB_MOCK_NOW_SECONDS,
      },
    ],
  };
}

export const graphHandlers = {
  load_workspace_project: (args: {
    project_id?: string | null;
    workspace_id?: string | null;
  }) => {
    const envelope = getWebWorkspaceFromSnapshot();
    if (
      !envelope.project ||
      (args.project_id && envelope.project.summary.projectId !== args.project_id)
    ) {
      return {
        project: null,
        workspace_id: envelope.workspace_id,
        sources: envelope.sources,
      };
    }
    return { ...envelope };
  },
  load_materialized_graph_snapshot: () => createWebMaterializedGraphSnapshot(),
  read_source_detail: (args: SourceDetailRequest) => ({
    sourceId: args.sourceId,
    fileName: args.originalPath.split(/[\\/]/).pop() ?? "sample.pdf",
    format: args.format,
    originalPath: args.originalPath,
    sourcePath: args.sourcePath,
    markdownPath: args.markdownPath,
    original: {
      kind: "unsupported" as const,
      previewUrl: null,
      text: null,
      truncated: false,
      error: "Original file preview is only available in the Electron runtime.",
    },
    markdown: {
      text: WEB_MOCK_MARKDOWN,
      missing: false,
      error: null,
    },
  }),
};
