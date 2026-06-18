import { type ReactNode, useEffect, useMemo, useState } from "react";
import {
  ArrowUp,
  Bot,
  FileText,
  Globe2,
  MessageCircle,
  Network,
  Plus,
  Search,
  Sparkles,
} from "lucide-react";

import type {
  AgentChatAskPayload,
  AgentChatAskResult,
  AgentChatMessage,
  AgentChatScopeMode,
} from "@/appTypes";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
import { fileNameFromPath } from "@/features/workspace/pathUtils";
import type { WorkspaceProject, WorkspaceSourceSummary } from "@/features/workspace/types";

const STORAGE_KEY = "hyprduck.agentChatThreads.v1";
const STORAGE_VERSION = 1;
const AGENT_CHAT_SCHEMA_VERSION = "hyprduck.agent_chat.v1";

interface AgentThread {
  id: string;
  title: string;
  createdAt: number;
  updatedAt: number;
  messages: AgentChatMessage[];
}

interface StoredThreads {
  version: number;
  threads: AgentThread[];
  activeThreadId: string | null;
}

interface AgentChatWorkspaceProps {
  project: WorkspaceProject | null;
  sources: WorkspaceSourceSummary[];
  selectedNodeId: string | null;
  workspaceId: string;
  providerReady: boolean;
  onAskAgentChat: (request: AgentChatAskPayload) => Promise<AgentChatAskResult>;
  onOpenDocs: () => void;
}

export function AgentChatWorkspace(props: AgentChatWorkspaceProps) {
  const {
    project,
    providerReady,
    selectedNodeId,
    sources,
    workspaceId,
    onAskAgentChat,
    onOpenDocs,
  } = props;
  const [threads, setThreads] = useState<AgentThread[]>(() => loadStoredThreads().threads);
  const [activeThreadId, setActiveThreadId] = useState<string | null>(
    () => loadStoredThreads().activeThreadId,
  );
  const [input, setInput] = useState("");
  const [scopeMode, setScopeMode] = useState<AgentChatScopeMode>("all_docs");
  const [selectedSourceId, setSelectedSourceId] = useState<string>(() => sources[0]?.source_id ?? "");
  const [pending, setPending] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [resultsByMessageId, setResultsByMessageId] = useState<Record<string, AgentChatAskResult>>({});

  useEffect(() => {
    persistThreads({ version: STORAGE_VERSION, threads, activeThreadId });
  }, [activeThreadId, threads]);

  useEffect(() => {
    if (!selectedSourceId && sources[0]?.source_id) {
      setSelectedSourceId(sources[0].source_id);
    }
  }, [selectedSourceId, sources]);

  const activeThread = useMemo(() => {
    if (!threads.length) {
      return null;
    }
    return threads.find((thread) => thread.id === activeThreadId) ?? threads[0] ?? null;
  }, [activeThreadId, threads]);

  const sourceIds = useMemo(() => {
    if (scopeMode === "selected_source") {
      return selectedSourceId ? [selectedSourceId] : [];
    }
    return sources.map((source) => source.source_id);
  }, [scopeMode, selectedSourceId, sources]);

  const canSend =
    input.trim().length > 0 &&
    !pending &&
    providerReady &&
    sources.length > 0 &&
    (scopeMode !== "selected_source" || selectedSourceId.length > 0) &&
    (scopeMode !== "graph_context" || Boolean(selectedNodeId));

  const startThread = () => {
    const thread = createThread();
    setThreads((current) => [thread, ...current]);
    setActiveThreadId(thread.id);
    setError(null);
  };

  const send = async () => {
    const question = input.trim();
    if (!canSend || !question) {
      return;
    }
    const thread = activeThread ?? createThread(question);
    const userMessage = createMessage("user", question);
    const pendingMessage = createMessage("assistant", "Thinking with your indexed evidence...");
    const nextTitle = thread.messages.length === 0 ? titleFromQuestion(question) : thread.title;
    const nextThread: AgentThread = {
      ...thread,
      title: nextTitle,
      updatedAt: unixTimestamp(),
      messages: [...thread.messages, userMessage, pendingMessage],
    };

    setInput("");
    setError(null);
    setPending(true);
    setActiveThreadId(thread.id);
    setThreads((current) => upsertThread(current, nextThread));

    try {
      const result = await onAskAgentChat({
        schemaVersion: AGENT_CHAT_SCHEMA_VERSION,
        conversationId: thread.id,
        scope: { workspaceId },
        mode: scopeMode,
        selectedNodeId: scopeMode === "graph_context" ? selectedNodeId : null,
        sourceIds,
        question,
        history: thread.messages,
        budget: 8_000,
        persistContextPack: true,
      });
      setResultsByMessageId((current) => ({
        ...current,
        [result.assistantMessage.id]: result,
      }));
      setThreads((current) =>
        upsertThread(
          current,
          replaceMessage(nextThread, pendingMessage.id, {
            ...result.assistantMessage,
            text: result.assistantMessage.text || result.answer.text || "",
          }),
        ),
      );
    } catch (sendError) {
      const message = createMessage("assistant", `Agent chat failed: ${String(sendError)}`);
      setError(String(sendError));
      setThreads((current) =>
        upsertThread(current, replaceMessage(nextThread, pendingMessage.id, message)),
      );
    } finally {
      setPending(false);
    }
  };

  return (
    <div className="grid min-h-0 flex-1 grid-cols-[16rem_minmax(0,1fr)] bg-background pt-12">
      <aside className="flex min-h-0 flex-col border-r border-border bg-muted/20 px-3 py-3">
        <Button className="justify-start gap-2" onClick={startThread} type="button" variant="secondary">
          <Plus size={16} />
          New chat
        </Button>
        <div className="mt-4 flex items-center gap-2 rounded-md border border-border bg-background px-3 py-2 text-sm text-muted-foreground">
          <Search size={15} />
          <span>Search chats</span>
        </div>
        <div className="mt-4 min-h-0 flex-1 overflow-y-auto">
          <div className="mb-2 px-1 text-xs font-medium uppercase tracking-wide text-muted-foreground">
            Chats
          </div>
          {threads.length === 0 ? (
            <p className="px-1 text-sm text-muted-foreground">No chats yet</p>
          ) : (
            <div className="space-y-1">
              {threads.map((thread) => (
                <button
                  className={cn(
                    "flex w-full items-center justify-between gap-2 rounded-md px-2 py-2 text-left text-sm",
                    activeThread?.id === thread.id
                      ? "bg-background text-foreground shadow-sm"
                      : "text-muted-foreground hover:bg-background hover:text-foreground",
                  )}
                  key={thread.id}
                  onClick={() => setActiveThreadId(thread.id)}
                  type="button"
                >
                  <span className="min-w-0 truncate">{thread.title}</span>
                  <span className="shrink-0 text-xs text-muted-foreground">{relativeAge(thread.updatedAt)}</span>
                </button>
              ))}
            </div>
          )}
        </div>
      </aside>

      <section className="flex min-h-0 flex-col overflow-hidden">
        <div className="flex min-h-0 flex-1 flex-col items-center overflow-y-auto px-6 pb-8">
          <div className="flex min-h-[18rem] w-full max-w-3xl flex-1 flex-col justify-center">
            {activeThread && activeThread.messages.length > 0 ? (
              <div className="mb-8 space-y-4">
                {activeThread.messages.map((message) => (
                  <MessageBubble
                    key={message.id}
                    message={message}
                    pending={pending && message.text.startsWith("Thinking")}
                    result={resultsByMessageId[message.id]}
                  />
                ))}
              </div>
            ) : (
              <div className="mb-8 text-center">
                <div className="mx-auto mb-5 flex size-11 items-center justify-center rounded-full border border-border bg-background">
                  <Sparkles size={18} />
                </div>
                <h1 className="text-2xl font-semibold text-foreground">What should we work on?</h1>
              </div>
            )}

            <div className="rounded-xl border border-border bg-background shadow-sm">
              <Textarea
                className="min-h-20 resize-none border-0 bg-transparent p-4 shadow-none focus-visible:ring-0"
                onChange={(event) => setInput(event.target.value)}
                onKeyDown={(event) => {
                  if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
                    event.preventDefault();
                    void send();
                  }
                }}
                placeholder="Ask anything about your indexed docs"
                value={input}
              />
              <div className="flex flex-wrap items-center justify-between gap-3 border-t border-border bg-muted/20 px-3 py-2">
                <div className="flex flex-wrap items-center gap-2">
                  <ScopeButton active={scopeMode === "all_docs"} icon={<Globe2 size={14} />} onClick={() => setScopeMode("all_docs")}>
                    All docs
                  </ScopeButton>
                  <ScopeButton
                    active={scopeMode === "selected_source"}
                    icon={<FileText size={14} />}
                    onClick={() => setScopeMode("selected_source")}
                  >
                    Selected source
                  </ScopeButton>
                  <ScopeButton
                    active={scopeMode === "graph_context"}
                    icon={<Network size={14} />}
                    onClick={() => setScopeMode("graph_context")}
                  >
                    Graph context
                  </ScopeButton>
                  {scopeMode === "selected_source" && (
                    <select
                      className="h-8 max-w-[14rem] rounded-md border border-border bg-background px-2 text-xs text-foreground"
                      onChange={(event) => setSelectedSourceId(event.target.value)}
                      value={selectedSourceId}
                    >
                      {sources.map((source) => (
                        <option key={source.source_id} value={source.source_id}>
                          {fileNameFromPath(source.original_path || source.source_path)}
                        </option>
                      ))}
                    </select>
                  )}
                </div>
                <Button disabled={!canSend} onClick={() => void send()} size="icon" type="button">
                  <ArrowUp size={16} />
                </Button>
              </div>
            </div>

            <div className="mt-5 space-y-1">
              {sources.length === 0 ? (
                <button
                  className="flex w-full items-center gap-2 border-b border-border px-3 py-2 text-left text-sm text-muted-foreground hover:text-foreground"
                  onClick={onOpenDocs}
                  type="button"
                >
                  <FileText size={16} />
                  Add docs before asking the agent
                </button>
              ) : (
                suggestedPrompts(project).map((prompt) => (
                  <button
                    className="flex w-full items-center gap-2 border-b border-border px-3 py-2 text-left text-sm text-muted-foreground hover:text-foreground"
                    key={prompt}
                    onClick={() => setInput(prompt)}
                    type="button"
                  >
                    <MessageCircle size={16} />
                    {prompt}
                  </button>
                ))
              )}
            </div>

            {error ? <p className="mt-3 text-sm text-destructive">{error}</p> : null}
            {!providerReady ? (
              <p className="mt-3 text-sm text-muted-foreground">
                Configure OpenRouter or Ollama in Settings before asking the agent.
              </p>
            ) : null}
          </div>
        </div>
      </section>
    </div>
  );
}

function MessageBubble({
  message,
  pending,
  result,
}: {
  message: AgentChatMessage;
  pending: boolean;
  result?: AgentChatAskResult;
}) {
  const isUser = message.role === "user";
  return (
    <div className={cn("flex", isUser ? "justify-end" : "justify-start")}>
      <div
        className={cn(
          "max-w-[min(42rem,88%)] rounded-lg border px-4 py-3 text-sm leading-6",
          isUser
            ? "border-primary bg-primary text-primary-foreground"
            : "border-border bg-muted/20 text-foreground",
        )}
      >
        <div className="mb-1 flex items-center gap-2 text-xs opacity-75">
          {isUser ? <MessageCircle size={13} /> : <Bot size={13} />}
          <span>{isUser ? "You" : "Agent"}</span>
          {pending ? <Badge variant="secondary">Pending</Badge> : null}
        </div>
        <p className="whitespace-pre-wrap">{message.text}</p>
        {result?.citations.length ? (
          <div className="mt-3 space-y-1 border-t border-border pt-2">
            {result.citations.slice(0, 3).map((citation) => (
              <div className="text-xs text-muted-foreground" key={citation.evidenceRef}>
                [{citation.evidenceRef}] {citation.quotedText.slice(0, 160)}
              </div>
            ))}
          </div>
        ) : null}
      </div>
    </div>
  );
}

function ScopeButton({
  active,
  children,
  icon,
  onClick,
}: {
  active: boolean;
  children: string;
  icon: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      className={cn(
        "inline-flex h-8 items-center gap-1.5 rounded-md border px-2 text-xs font-medium",
        active
          ? "border-border bg-background text-foreground shadow-sm"
          : "border-transparent text-muted-foreground hover:bg-background hover:text-foreground",
      )}
      onClick={onClick}
      type="button"
    >
      {icon}
      {children}
    </button>
  );
}

function loadStoredThreads(): StoredThreads {
  if (typeof window === "undefined") {
    return emptyStorage();
  }
  try {
    const raw = window.localStorage.getItem(STORAGE_KEY);
    if (!raw) {
      return emptyStorage();
    }
    const parsed = JSON.parse(raw) as StoredThreads;
    if (parsed.version !== STORAGE_VERSION || !Array.isArray(parsed.threads)) {
      return emptyStorage();
    }
    return {
      version: STORAGE_VERSION,
      threads: parsed.threads,
      activeThreadId: parsed.activeThreadId ?? parsed.threads[0]?.id ?? null,
    };
  } catch {
    return emptyStorage();
  }
}

function persistThreads(value: StoredThreads) {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
}

function emptyStorage(): StoredThreads {
  return { version: STORAGE_VERSION, threads: [], activeThreadId: null };
}

function createThread(seed?: string): AgentThread {
  const now = unixTimestamp();
  return {
    id: `chat_${crypto.randomUUID()}`,
    title: seed ? titleFromQuestion(seed) : "New chat",
    createdAt: now,
    updatedAt: now,
    messages: [],
  };
}

function createMessage(role: AgentChatMessage["role"], text: string): AgentChatMessage {
  return {
    id: `msg_${crypto.randomUUID()}`,
    role,
    text,
    createdAt: unixTimestamp(),
  };
}

function upsertThread(threads: AgentThread[], thread: AgentThread): AgentThread[] {
  const withoutThread = threads.filter((item) => item.id !== thread.id);
  return [thread, ...withoutThread].sort((a, b) => b.updatedAt - a.updatedAt);
}

function replaceMessage(
  thread: AgentThread,
  messageId: string,
  replacement: AgentChatMessage,
): AgentThread {
  return {
    ...thread,
    updatedAt: unixTimestamp(),
    messages: thread.messages.map((message) =>
      message.id === messageId ? replacement : message,
    ),
  };
}

function suggestedPrompts(project: WorkspaceProject | null): string[] {
  const title = project?.summary.title ?? "these docs";
  return [
    `Summarize the most important evidence in ${title}`,
    "Find unresolved decisions and cite the source evidence",
    "What should I verify in the graph before reusing this context?",
  ];
}

function titleFromQuestion(question: string) {
  return question.length > 34 ? `${question.slice(0, 34)}...` : question;
}

function relativeAge(timestamp: number) {
  const seconds = Math.max(0, unixTimestamp() - timestamp);
  if (seconds < 60) {
    return "now";
  }
  if (seconds < 3600) {
    return `${Math.floor(seconds / 60)}m`;
  }
  if (seconds < 86_400) {
    return `${Math.floor(seconds / 3600)}h`;
  }
  return `${Math.floor(seconds / 86_400)}d`;
}

function unixTimestamp() {
  return Math.floor(Date.now() / 1000);
}
