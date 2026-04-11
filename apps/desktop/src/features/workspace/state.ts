import type { WorkspaceProject } from "./types";

export interface WorkspaceUiState {
  selectedNodeId: string | null;
  inspectorOpen: boolean;
  answerDockOpen: boolean;
  answerInput: string;
}

export type WorkspaceUiAction =
  | { type: "sync_project"; project: WorkspaceProject | null }
  | { type: "select_node"; nodeId: string }
  | { type: "toggle_inspector" }
  | { type: "open_answer_dock" }
  | { type: "close_answer_dock" }
  | { type: "set_answer_input"; value: string };

export function createInitialWorkspaceUiState(
  project: WorkspaceProject | null,
): WorkspaceUiState {
  return {
    selectedNodeId: defaultSelectedNodeId(project),
    inspectorOpen: true,
    answerDockOpen: false,
    answerInput: "",
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
      return {
        ...state,
        selectedNodeId,
      };
    }
    case "select_node":
      return {
        ...state,
        selectedNodeId: action.nodeId,
        inspectorOpen: true,
      };
    case "toggle_inspector":
      return {
        ...state,
        inspectorOpen: !state.inspectorOpen,
      };
    case "open_answer_dock":
      return {
        ...state,
        answerDockOpen: true,
      };
    case "close_answer_dock":
      return {
        ...state,
        answerDockOpen: false,
      };
    case "set_answer_input":
      return {
        ...state,
        answerInput: action.value,
      };
    default:
      return state;
  }
}

function defaultSelectedNodeId(project: WorkspaceProject | null): string | null {
  if (!project) {
    return null;
  }

  const nonDocumentNode = project.nodes.find((node) => node.kind !== "document");
  return nonDocumentNode?.id ?? project.nodes[0]?.id ?? null;
}
