import type {
  AgentChatStreamEvent,
  DesktopCommand,
  DesktopCommandArgs,
  DesktopCommandParameters,
  DesktopCommandResult,
  DesktopMessage,
  EtymaDesktopApi,
  UiSnapshot,
} from "@/appTypes";

import { agentChatHandlers } from "./webPreview/handlers/agentChat";
import { configHandlers, deriveWebValidation } from "./webPreview/handlers/config";
import { graphHandlers } from "./webPreview/handlers/graph";
import { parseHandlers } from "./webPreview/handlers/parse";
import { snapshotHandlers } from "./webPreview/handlers/snapshot";
import { makeUnsupportedHandler } from "./webPreview/handlers/unsupported";
import {
  setWebMockValidation,
  webMockAgentChatListeners,
  webMockSnapshotListeners,
} from "./webPreview/state";

type WebPreviewCommandHandlers = {
  [K in DesktopCommand]: (
    args: DesktopCommandArgs<K>,
  ) => DesktopCommandResult<K> | Promise<DesktopCommandResult<K>>;
};

/**
 * Browser web-preview mock API.
 * Demo-critical surfaces: snapshot, parse fake, load graph, agent chat, config.
 * Other desktop commands throw a clear unsupported error.
 */
export function createWebMockApi(): EtymaDesktopApi {
  setWebMockValidation(deriveWebValidation(null));

  const handlers: WebPreviewCommandHandlers = {
    app_snapshot: snapshotHandlers.app_snapshot,
    load_engine_config: configHandlers.load_engine_config,
    validate_engine_config: configHandlers.validate_engine_config,
    engine_readiness: configHandlers.engine_readiness,
    get_models_for_provider: configHandlers.get_models_for_provider,
    save_engine_config: configHandlers.save_engine_config,
    load_workspace_project: graphHandlers.load_workspace_project,
    load_materialized_graph_snapshot: graphHandlers.load_materialized_graph_snapshot,
    read_source_detail: graphHandlers.read_source_detail,
    pick_import_file: parseHandlers.pick_import_file,
    start_parse: parseHandlers.start_parse,
    retry_failed_pages: parseHandlers.retry_failed_pages,
    cancel_parse: parseHandlers.cancel_parse,
    agent_chat_ask: agentChatHandlers.agent_chat_ask,
    agent_chat_start: agentChatHandlers.agent_chat_start,
    agent_chat_stop: agentChatHandlers.agent_chat_stop,
    // Non-demo surfaces: clear unsupported errors (no elaborate mocks).
    open_saved_output: makeUnsupportedHandler("open_saved_output"),
    open_local_artifact: makeUnsupportedHandler("open_local_artifact"),
    apply_workspace_correction: makeUnsupportedHandler("apply_workspace_correction"),
    agent_terminal_list_agents: makeUnsupportedHandler("agent_terminal_list_agents"),
    agent_terminal_create_session: makeUnsupportedHandler("agent_terminal_create_session"),
    agent_terminal_snapshot_session: makeUnsupportedHandler("agent_terminal_snapshot_session"),
    agent_terminal_write_session: makeUnsupportedHandler("agent_terminal_write_session"),
    agent_terminal_resize_session: makeUnsupportedHandler("agent_terminal_resize_session"),
    agent_terminal_kill_session: makeUnsupportedHandler("agent_terminal_kill_session"),
  };

  return {
    async invoke<K extends DesktopCommand>(
      command: K,
      ...args: DesktopCommandParameters<K>
    ): Promise<DesktopCommandResult<K>> {
      const handler = handlers[command] as (
        args: DesktopCommandArgs<K>,
      ) => DesktopCommandResult<K> | Promise<DesktopCommandResult<K>>;
      return handler(args[0] as DesktopCommandArgs<K>);
    },
    listen<T>(
      eventName: string,
      handler: (message: DesktopMessage<T>) => void | Promise<void>,
    ) {
      if (eventName === "etyma://agent-terminal") {
        return () => undefined;
      }
      if (eventName === "etyma://agent-chat") {
        const typedHandler = (message: DesktopMessage<AgentChatStreamEvent>) => {
          void handler(message as DesktopMessage<T>);
        };
        webMockAgentChatListeners.add(typedHandler);
        return () => {
          webMockAgentChatListeners.delete(typedHandler);
        };
      }
      if (eventName !== "etyma://snapshot") {
        return () => undefined;
      }
      const typedHandler = (message: DesktopMessage<UiSnapshot>) => {
        void handler(message as DesktopMessage<T>);
      };
      webMockSnapshotListeners.add(typedHandler);
      return () => {
        webMockSnapshotListeners.delete(typedHandler);
      };
    },
  };
}
