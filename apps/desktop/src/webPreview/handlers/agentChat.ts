import type {
  AgentChatAskPayload,
  AgentChatAskResult,
  AgentChatStreamEvent,
} from "@/appTypes";

import { getWebWorkspaceFromSnapshot } from "./graph";
import {
  emitWebAgentChatEvent,
  webMockAgentChatTimers,
  webMockConfig,
} from "../state";

function createWebAgentChatResult(request: AgentChatAskPayload): AgentChatAskResult {
  const workspace = getWebWorkspaceFromSnapshot();
  if (!workspace.project) {
    throw new Error("No workspace available in preview mode.");
  }
  const answer =
    workspace.project.answerByNodeId["source:preview"] ??
    Object.values(workspace.project.answerByNodeId)[0];
  if (!answer) {
    throw new Error("No answer available for this workspace in preview mode.");
  }
  const now = Math.floor(Date.now() / 1000);
  return {
    schemaVersion: "hyprduck.agent_chat.v1",
    conversationId: request.conversationId,
    answerMode: "evidence",
    assistantMessage: {
      id: request.assistantMessageId ?? `preview-msg-${now}`,
      role: "assistant",
      text:
        answer.text ??
        "This preview answer is grounded in the sample workspace evidence.",
      createdAt: now,
    },
    answer,
    contextPackId: "ctx_web_preview",
    persistedContextPackPath: null,
    citations: answer.citations.map((citation) => ({
      evidenceRef: citation.id,
      sourceId: citation.sourceId ?? "preview",
      page: citation.pageIndex ?? 1,
      region: null,
      span: null,
      quotedText: citation.snippet,
      parseConfidence: "high",
      selectionReason: citation.provenance ?? "preview evidence",
      contentHash: "web-preview",
      evidenceType: "text",
    })),
    retrievalTrace: {
      strategy: "web_preview",
      chunksConsidered: answer.citations.length,
      chunksSelected: answer.citations.length,
      budgetRequested: request.budget ?? 8000,
      budgetUsed: answer.citations.length * 64,
      evidenceTypeTrace: {
        considered: { text: answer.citations.length },
        selected: { text: answer.citations.length },
      },
    },
    provider: {
      id: webMockConfig.provider,
      label: webMockConfig.provider === "open_router" ? "OpenRouter" : "Ollama",
      modelId: webMockConfig.model_id,
      hosted: webMockConfig.provider === "open_router",
    },
    warnings: [],
  };
}

export const agentChatHandlers = {
  agent_chat_ask: (args: { request: AgentChatAskPayload }) =>
    createWebAgentChatResult(args.request),
  agent_chat_start: (args: { request: AgentChatAskPayload }) => {
    const requestId = `preview-agent-${Date.now()}`;
    const conversationId = args.request.conversationId;
    const assistantMessageId =
      args.request.assistantMessageId ?? `preview-msg-${Date.now()}`;
    const request = { ...args.request, assistantMessageId };
    const result = createWebAgentChatResult(request);
    const text = result.assistantMessage.text || result.answer.text || "";
    const chunks = text.match(/.{1,32}/g) ?? [text];
    const timers: ReturnType<typeof setTimeout>[] = [];
    const schedule = (delay: number, event: AgentChatStreamEvent) => {
      timers.push(setTimeout(() => emitWebAgentChatEvent(event), delay));
    };
    schedule(10, {
      requestId,
      type: "started",
      conversationId,
      assistantMessageId,
      provider: result.provider,
      answerMode: result.answerMode,
    });
    schedule(40, {
      requestId,
      type: "status",
      status: "retrieving_context",
      message: "Retrieving context...",
    });
    schedule(90, {
      requestId,
      type: "status",
      status: "generating",
      message: "Generating...",
    });
    chunks.forEach((chunk, index) => {
      schedule(130 + index * 45, {
        requestId,
        type: "delta",
        text: chunk,
      });
    });
    const finalDelay = 160 + chunks.length * 45;
    schedule(finalDelay, {
      requestId,
      type: "status",
      status: "validating_citations",
      message: "Validating citations...",
    });
    schedule(finalDelay + 40, {
      requestId,
      type: "final",
      result,
    });
    timers.push(
      setTimeout(() => {
        webMockAgentChatTimers.delete(requestId);
      }, finalDelay + 60),
    );
    webMockAgentChatTimers.set(requestId, timers);
    return { requestId, conversationId, assistantMessageId };
  },
  agent_chat_stop: (args: { requestId: string }) => {
    const timers = webMockAgentChatTimers.get(args.requestId) ?? [];
    for (const timer of timers) {
      clearTimeout(timer);
    }
    webMockAgentChatTimers.delete(args.requestId);
    emitWebAgentChatEvent({
      requestId: args.requestId,
      type: "stopped",
      partialText: "",
    });
    return { stopped: timers.length > 0 };
  },
};
