const crypto = require("node:crypto");

const AGENT_CHAT_EVENT = "hyprduck://agent-chat";

function createAgentChatStream({
  getMainWindow,
  snapshot,
  runEngineCommand,
  resetEngineRuntime,
  brainReadScope,
}) {
  const activeAgentChatStreams = new Map();

  function publishAgentChatEvent(payload) {
    const mainWindow = getMainWindow();
    if (!mainWindow || mainWindow.isDestroyed()) {
      return;
    }
    mainWindow.webContents.send(AGENT_CHAT_EVENT, payload);
  }

  function startAgentChat(request) {
    const requestId = `agent_${crypto.randomUUID()}`;
    const conversationId = request.conversationId || `chat_${crypto.randomUUID()}`;
    const assistantMessageId =
      request.assistantMessageId || `msg_${crypto.randomUUID().replaceAll("-", "")}`;
    const workspaceId = request.scope?.workspaceId ?? snapshot.lastWorkspaceId ?? "default";
    const payload = {
      ...request,
      conversationId,
      assistantMessageId,
      scope: brainReadScope(workspaceId),
    };
    const streamState = { requestId, stopped: false };
    activeAgentChatStreams.set(requestId, streamState);

    setImmediate(() => {
      if (!activeAgentChatStreams.has(requestId)) {
        return;
      }
      void runEngineCommand(
        "agent_chat_ask",
        {
          command: "agent_chat_ask",
          payload,
        },
        {
          onEvent: (event) => {
            if (!activeAgentChatStreams.has(requestId)) {
              return;
            }
            if (!event || typeof event !== "object") {
              return;
            }
            publishAgentChatEvent({ requestId, ...event });
          },
        },
      )
        .catch((error) => {
          const active = activeAgentChatStreams.get(requestId);
          if (!active || active.stopped) {
            return;
          }
          publishAgentChatEvent({
            requestId,
            type: "error",
            code: error.code ?? "runtime_error",
            message: error.message,
          });
        })
        .finally(() => {
          activeAgentChatStreams.delete(requestId);
        });
    });

    return { requestId, conversationId, assistantMessageId };
  }

  function stopAgentChat(requestId) {
    if (!requestId || !activeAgentChatStreams.has(requestId)) {
      return { stopped: false };
    }
    const active = activeAgentChatStreams.get(requestId);
    active.stopped = true;
    resetEngineRuntime();
    publishAgentChatEvent({
      requestId,
      type: "stopped",
      partialText: "",
    });
    activeAgentChatStreams.delete(requestId);
    return { stopped: true };
  }

  return {
    AGENT_CHAT_EVENT,
    startAgentChat,
    stopAgentChat,
  };
}

module.exports = {
  AGENT_CHAT_EVENT,
  createAgentChatStream,
};
