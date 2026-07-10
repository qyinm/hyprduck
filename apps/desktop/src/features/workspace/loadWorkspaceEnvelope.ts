import {
  type WorkspaceLoadResult,
  type WorkspaceLoadState,
} from "@/appTypes";
import { invoke } from "@/desktopApi";
import { materializedGraphSnapshotToWorkspaceEnvelope } from "@/features/workspace/materializedGraphSnapshot";
import type { WorkspaceProjectEnvelope } from "@/features/workspace/types";
import type { TranslationKey } from "@/i18n/locales";

export type { WorkspaceLoadResult, WorkspaceLoadState };

type TranslateFn = (
  key: TranslationKey,
  replacements?: Record<string, string | number>,
) => string;

/**
 * Load strategy (order preserved):
 * 1. load_materialized_graph_snapshot → source: "materialized"
 * 2. else load_workspace_project → source: "legacy" + fallbackReason
 * 3. else throw combined error
 */
export async function loadGraphWorkspaceEnvelope(
  workspaceId?: string | null,
  projectId?: string | null,
): Promise<WorkspaceProjectEnvelope> {
  return (await loadGraphWorkspaceEnvelopeResult(workspaceId, projectId)).envelope;
}

export async function loadGraphWorkspaceEnvelopeResult(
  workspaceId?: string | null,
  projectId?: string | null,
): Promise<WorkspaceLoadResult> {
  try {
    const materializedSnapshot = await invoke("load_materialized_graph_snapshot", {
      workspace_id: workspaceId ?? undefined,
    });
    return {
      envelope: materializedGraphSnapshotToWorkspaceEnvelope(materializedSnapshot),
      source: "materialized",
    };
  } catch (materializedError) {
    try {
      return {
        envelope: await invoke("load_workspace_project", {
          project_id: projectId ?? null,
          workspace_id: workspaceId ?? null,
        }),
        source: "legacy",
        fallbackReason: String(materializedError),
      };
    } catch (legacyError) {
      throw new Error(
        `Failed to refresh latest workspace snapshot. Materialized read failed: ${String(
          materializedError,
        )}. Legacy project read failed: ${String(legacyError)}.`,
      );
    }
  }
}

export function workspaceLoadStateFromResult(
  result: WorkspaceLoadResult,
  t: TranslateFn,
): WorkspaceLoadState {
  if (result.source === "materialized") {
    const snapshotPath =
      result.envelope.project?.summary.summary.match(/from (.+)\.$/)?.[1] ??
      "state/latest-readable-snapshot.json";
    return {
      status: "ready",
      message: t("workspace.status.materializedPrefix", { path: snapshotPath }),
    };
  }

  return {
    status: "fallback",
    message: t("workspace.status.legacyFallback"),
  };
}
