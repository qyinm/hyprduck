import type { WorkspaceProject } from "./types";

export interface WorkspaceUiState {
  selectedNodeId: string | null;
  selectedEdgeId: string | null;
  inspectorOpen: boolean;
}

export type WorkspaceUiAction =
  | { type: "sync_project"; project: WorkspaceProject | null }
  | { type: "select_node"; nodeId: string }
  | { type: "select_edge"; edgeId: string }
  | { type: "toggle_inspector" };

export function createInitialWorkspaceUiState(
  project: WorkspaceProject | null,
): WorkspaceUiState {
  return {
    selectedNodeId: defaultSelectedNodeId(project),
    selectedEdgeId: null,
    inspectorOpen: false,
  };
}

export function workspaceUiStateReducer(
  state: WorkspaceUiState,
  action: WorkspaceUiAction,
): WorkspaceUiState {
  switch (action.type) {
    case "sync_project": {
      const selectedNodeId =
        action.project &&
        state.selectedNodeId &&
        action.project.detailsByNodeId[state.selectedNodeId]
          ? state.selectedNodeId
          : defaultSelectedNodeId(action.project);
      const selectedEdgeId =
        action.project &&
        state.selectedEdgeId &&
        action.project.edgeDetailsById[state.selectedEdgeId]
          ? state.selectedEdgeId
          : null;
      return {
        ...state,
        selectedNodeId,
        selectedEdgeId,
      };
    }
    case "select_node":
      return {
        ...state,
        selectedNodeId: action.nodeId,
        selectedEdgeId: null,
        inspectorOpen: true,
      };
    case "select_edge":
      return {
        ...state,
        selectedNodeId: null,
        selectedEdgeId: action.edgeId,
        inspectorOpen: true,
      };
    case "toggle_inspector":
      return {
        ...state,
        inspectorOpen: !state.inspectorOpen,
      };
    default:
      return state;
  }
}

function defaultSelectedNodeId(project: WorkspaceProject | null): string | null {
  if (!project) {
    return null;
  }

  const nonSourceNode = project.nodes.find(
    (node) => node.kind !== "source" && node.kind !== "document",
  );
  return nonSourceNode?.id ?? project.nodes[0]?.id ?? null;
}
