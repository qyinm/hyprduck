import { type ComponentProps, type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import type { Components } from "streamdown";
import {
  ArrowUp,
  Bot,
  FileText,
  MessageCircle,
  Plus,
  Search,
  Sparkles,
  Square,
} from "lucide-react";

import type {
  AgentChatAskPayload,
  AgentChatAskResult,
  AgentChatCitation,
  AgentChatMessage,
  AgentChatStartResult,
  AgentChatStreamEvent,
  AgentChatStreamStatus,
  DesktopMessage,
  DesktopUnlisten,
} from "@/appTypes";
import {
  InlineCitation,
  InlineCitationCard,
  InlineCitationCardBody,
  InlineCitationCardTrigger,
  InlineCitationQuote,
  InlineCitationSource,
} from "@/components/ai-elements/inline-citation";
import {
  Message,
  MessageContent,
  MessageResponse,
} from "@/components/ai-elements/message";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";
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
  resultsByMessageId: Record<string, AgentChatAskResult>;
}

interface AgentChatWorkspaceProps {
  project: WorkspaceProject | null;
  sources: WorkspaceSourceSummary[];
  selectedNodeId: string | null;
  workspaceId: string;
  providerReady: boolean;
  onListenAgentChatEvents: (
    handler: (message: DesktopMessage<AgentChatStreamEvent>) => void | Promise<void>,
  ) => DesktopUnlisten;
  onOpenDocs: () => void;
  onStartAgentChat: (request: AgentChatAskPayload) => Promise<AgentChatStartResult>;
  onStopAgentChat: (requestId: string) => Promise<{ stopped: boolean }>;
}

interface ActiveStreamState {
  requestId: string;
  threadId: string;
  assistantMessageId: string;
}

interface MessageStatusState {
  status: AgentChatStreamStatus;
  message: string;
}

export function AgentChatWorkspace(props: AgentChatWorkspaceProps) {
  const {
    project,
    providerReady,
    sources,
    workspaceId,
    onListenAgentChatEvents,
    onOpenDocs,
    onStartAgentChat,
    onStopAgentChat,
  } = props;
  const [storedThreads] = useState<StoredThreads>(() => loadStoredThreads());
  const [threads, setThreads] = useState<AgentThread[]>(() => storedThreads.threads);
  const [activeThreadId, setActiveThreadId] = useState<string | null>(
    () => storedThreads.activeThreadId,
  );
  const [input, setInput] = useState("");
  const [starting, setStarting] = useState(false);
  const [activeStream, setActiveStream] = useState<ActiveStreamState | null>(null);
  const activeStreamRef = useRef<ActiveStreamState | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [resultsByMessageId, setResultsByMessageId] = useState<Record<string, AgentChatAskResult>>(
    () => storedThreads.resultsByMessageId,
  );
  const [streamStatusByMessageId, setStreamStatusByMessageId] = useState<
    Record<string, MessageStatusState>
  >({});
  const [stoppedMessageIds, setStoppedMessageIds] = useState<Record<string, boolean>>({});

  useEffect(() => {
    activeStreamRef.current = activeStream;
  }, [activeStream]);

  useEffect(() => {
    persistThreads({ version: STORAGE_VERSION, threads, activeThreadId, resultsByMessageId });
  }, [activeThreadId, resultsByMessageId, threads]);

  useEffect(() => {
    return onListenAgentChatEvents((message) => {
      const event = message.payload;
      const stream = activeStreamRef.current;
      if (!stream || event.requestId !== stream.requestId) {
        return;
      }

      if (event.type === "status") {
        setStreamStatusByMessageId((current) => ({
          ...current,
          [stream.assistantMessageId]: {
            status: event.status,
            message: statusLabel(event.status, event.message),
          },
        }));
        return;
      }

      if (event.type === "delta") {
        setThreads((current) =>
          updateThreadMessage(current, stream.threadId, stream.assistantMessageId, (existing) => ({
            ...existing,
            text: `${existing.text}${event.text}`,
          })),
        );
        return;
      }

      if (event.type === "final") {
        const assistantMessage = {
          ...event.result.assistantMessage,
          id: stream.assistantMessageId,
          text: event.result.assistantMessage.text || event.result.answer.text || "",
        };
        const result = {
          ...event.result,
          assistantMessage,
        };
        setResultsByMessageId((current) => ({
          ...current,
          [assistantMessage.id]: result,
        }));
        setThreads((current) =>
          updateThreadMessage(current, stream.threadId, stream.assistantMessageId, () => assistantMessage),
        );
        setStreamStatusByMessageId((current) => removeKey(current, stream.assistantMessageId));
        setStoppedMessageIds((current) => removeKey(current, stream.assistantMessageId));
        setActiveStream(null);
        setError(null);
        return;
      }

      if (event.type === "error") {
        const text = `Agent chat failed: ${event.message}`;
        setError(event.message);
        setThreads((current) =>
          updateThreadMessage(current, stream.threadId, stream.assistantMessageId, (existing) => ({
            ...existing,
            text: existing.text || text,
          })),
        );
        setStreamStatusByMessageId((current) => removeKey(current, stream.assistantMessageId));
        setActiveStream(null);
        return;
      }

      if (event.type === "stopped") {
        setStoppedMessageIds((current) => ({
          ...current,
          [stream.assistantMessageId]: true,
        }));
        setStreamStatusByMessageId((current) => removeKey(current, stream.assistantMessageId));
        setActiveStream(null);
      }
    });
  }, [onListenAgentChatEvents]);

  const activeThread = useMemo(() => {
    if (!threads.length) {
      return null;
    }
    return threads.find((thread) => thread.id === activeThreadId) ?? threads[0] ?? null;
  }, [activeThreadId, threads]);
  const hasConversation = Boolean(activeThread?.messages.length);

  const canSend =
    input.trim().length > 0 && !starting && !activeStream && providerReady;

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
    const assistantMessage = createMessage("assistant", "");
    const nextTitle = thread.messages.length === 0 ? titleFromQuestion(question) : thread.title;
    const nextThread: AgentThread = {
      ...thread,
      title: nextTitle,
      updatedAt: unixTimestamp(),
      messages: [...thread.messages, userMessage, assistantMessage],
    };

    setInput("");
    setError(null);
    setStarting(true);
    setActiveThreadId(thread.id);
    setThreads((current) => upsertThread(current, nextThread));
    setStreamStatusByMessageId((current) => ({
      ...current,
      [assistantMessage.id]: {
        status: "resolving_scope",
        message: "Resolving scope...",
      },
    }));
    setStoppedMessageIds((current) => removeKey(current, assistantMessage.id));

    try {
      const started = await onStartAgentChat({
        schemaVersion: AGENT_CHAT_SCHEMA_VERSION,
        conversationId: thread.id,
        assistantMessageId: assistantMessage.id,
        scope: { workspaceId },
        mode: "auto",
        selectedNodeId: null,
        sourceIds: sources.map((source) => source.source_id),
        question,
        history: thread.messages,
        budget: 8_000,
        persistContextPack: true,
      });
      setActiveStream({
        requestId: started.requestId,
        threadId: thread.id,
        assistantMessageId: started.assistantMessageId,
      });
    } catch (sendError) {
      const message = `Agent chat failed: ${String(sendError)}`;
      setError(message);
      setThreads((current) =>
        updateThreadMessage(current, thread.id, assistantMessage.id, (existing) => ({
          ...existing,
          text: message,
        })),
      );
      setStreamStatusByMessageId((current) => removeKey(current, assistantMessage.id));
    } finally {
      setStarting(false);
    }
  };

  const stop = async () => {
    const stream = activeStreamRef.current;
    if (!stream) {
      return;
    }
    await onStopAgentChat(stream.requestId);
  };

  const composer = (placeholder: string) => (
    <div className="rounded-2xl border border-border bg-background p-3 shadow-sm">
      <Textarea
        className="min-h-24 resize-none border-0 bg-transparent px-2 pb-2 pt-1 text-[15px] shadow-none focus-visible:ring-0"
        onChange={(event) => setInput(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === "Enter" && (event.metaKey || event.ctrlKey)) {
            event.preventDefault();
            void send();
          }
        }}
        placeholder={placeholder}
        rows={2}
        value={input}
      />
      <div className="flex items-center justify-end pt-1">
        <Button
          className="rounded-full"
          disabled={!activeStream && !canSend}
          onClick={() => (activeStream ? void stop() : void send())}
          size="icon"
          type="button"
        >
          {activeStream ? <Square size={14} /> : <ArrowUp size={16} />}
        </Button>
      </div>
    </div>
  );

  return (
    <div className="grid min-h-0 flex-1 grid-cols-[16rem_minmax(0,1fr)] bg-background">
      <aside className="flex min-h-0 flex-col border-r border-border bg-muted/20 px-3 pb-3 pt-14">
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

      <section className="flex min-h-0 flex-col overflow-hidden pt-12">
        {hasConversation ? (
          <div className="flex min-h-0 flex-1 flex-col items-center overflow-hidden px-6 pb-5">
            <div className="flex min-h-0 w-full max-w-4xl flex-1 flex-col">
              <div className="min-h-0 flex-1 overflow-y-auto pb-6 pt-4">
                <div className="space-y-6">
                  {activeThread?.messages.map((message) => (
                    <MessageBubble
                      key={message.id}
                      message={message}
                      result={resultsByMessageId[message.id]}
                      status={streamStatusByMessageId[message.id]}
                      stopped={Boolean(stoppedMessageIds[message.id])}
                    />
                  ))}
                </div>
              </div>

              <div className="shrink-0 pb-1">
                {composer("Ask a follow-up about your indexed docs")}
                {error ? <p className="mt-3 text-sm text-destructive">{error}</p> : null}
                {!providerReady ? (
                  <p className="mt-3 text-sm text-muted-foreground">
                    Configure OpenRouter or Ollama in Settings before asking the agent.
                  </p>
                ) : null}
              </div>
            </div>
          </div>
        ) : (
          <div className="flex min-h-0 flex-1 flex-col items-center overflow-y-auto px-6 pb-8">
            <div className="flex min-h-[18rem] w-full max-w-3xl flex-1 flex-col justify-center">
              <div className="mb-8 text-center">
                <div className="mx-auto mb-5 flex size-11 items-center justify-center rounded-full border border-border bg-background">
                  <Sparkles size={18} />
                </div>
                <h1 className="text-2xl font-semibold text-foreground">What should we work on?</h1>
              </div>

              {composer("Ask anything about your indexed docs")}

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
        )}
      </section>
    </div>
  );
}

function MessageBubble({
  message,
  result,
  status,
  stopped,
}: {
  message: AgentChatMessage;
  result?: AgentChatAskResult;
  status?: MessageStatusState;
  stopped?: boolean;
}) {
  const isUser = message.role === "user";
  const citations = result?.citations ?? [];
  const assistantText = isUser ? message.text : formatAssistantDisplayText(message.text, citations);
  const citedAssistantText = isUser ? assistantText : linkifyCitationMarkers(
    ensureCitationMarkers(assistantText, citations),
    citations,
  );
  const citationComponents = useMemo(
    () => (citations.length ? createCitationComponents(citations) : undefined),
    [citations],
  );

  if (isUser) {
    return (
      <Message className="max-w-[min(32rem,78%)]" from="user">
        <MessageContent>
          <div className="flex items-center gap-2 text-xs opacity-75">
            <MessageCircle size={13} />
            <span>You</span>
          </div>
          <p className="whitespace-pre-wrap text-sm leading-6">{message.text}</p>
        </MessageContent>
      </Message>
    );
  }

  return (
    <Message className="max-w-[min(48rem,92%)]" from="assistant">
      <MessageContent className="w-full overflow-visible">
        <div className="flex items-center gap-2 text-xs font-medium text-muted-foreground">
          <Bot size={13} />
          <span>Agent</span>
          {status ? <Badge variant="secondary">{statusLabel(status.status, status.message)}</Badge> : null}
          {stopped ? <Badge variant="outline">Stopped</Badge> : null}
        </div>
        {status && !citedAssistantText ? (
          <div className="flex items-center gap-2 text-sm text-muted-foreground">
            <span className="size-1.5 animate-pulse rounded-full bg-muted-foreground" />
            <span>{statusLabel(status.status, status.message)}</span>
          </div>
        ) : (
          <>
            <MessageResponse components={citationComponents}>{citedAssistantText}</MessageResponse>
            {citations.length ? <CitationSources citations={citations} /> : null}
          </>
        )}
      </MessageContent>
    </Message>
  );
}

function createCitationComponents(citations: AgentChatCitation[]): Components {
  return {
    a: ({ href, children, node: _node, ...props }: ComponentProps<"a"> & { node?: unknown }) => {
      const marker = typeof href === "string" ? href.match(/^#citation-(\d+)$/) : null;
      if (marker) {
        const index = Number(marker[1]);
        const citation = citations[index - 1];
        if (citation) {
          return (
            <InlineCitationMarker citation={citation} index={index}>
              {children}
            </InlineCitationMarker>
          );
        }
      }
      return (
        <a
          className="underline underline-offset-2"
          href={href}
          rel="noreferrer"
          target={href?.startsWith("#") ? undefined : "_blank"}
          {...props}
        >
          {children}
        </a>
      );
    },
  };
}

function InlineCitationMarker({
  citation,
  children,
  index,
}: {
  citation: AgentChatCitation;
  children: ReactNode;
  index: number;
}) {
  const title = `Evidence ${index}`;
  const sourceLabel = citation.page > 0 ? `Page ${citation.page}` : "Source";
  return (
    <InlineCitation>
      <InlineCitationCard>
        <InlineCitationCardTrigger aria-label={`${title}: ${sourceLabel}`} sources={[sourceLabel]}>
          {children}
        </InlineCitationCardTrigger>
        <InlineCitationCardBody>
          <InlineCitationSource
            description={citation.selectionReason}
            title={title}
            url={`${sourceLabel} · ${shortEvidenceRef(citation.evidenceRef)}`}
          />
          <InlineCitationQuote className="mt-2 max-h-56 overflow-y-auto">
            {citation.quotedText}
          </InlineCitationQuote>
        </InlineCitationCardBody>
      </InlineCitationCard>
    </InlineCitation>
  );
}

function CitationSources({ citations }: { citations: AgentChatCitation[] }) {
  return (
    <section className="mt-3 border-t border-border pt-3" aria-label="Sources">
      <div className="mb-2 flex items-center gap-2 text-xs font-medium uppercase tracking-wide text-muted-foreground">
        <FileText size={13} />
        <span>Sources</span>
      </div>
      <div className="space-y-2">
        {citations.map((citation, index) => {
          const sourceLabel = citation.page > 0 ? `Page ${citation.page}` : "Source";
          return (
            <details
              className="rounded-lg border border-border bg-muted/20 px-3 py-2 text-sm"
              key={citation.evidenceRef}
            >
              <summary className="flex cursor-pointer list-none items-center justify-between gap-3 text-left">
                <span className="flex min-w-0 items-center gap-2">
                  <span className="inline-flex size-5 shrink-0 items-center justify-center rounded-full border border-border bg-background text-[11px] font-medium">
                    {index + 1}
                  </span>
                  <span className="font-medium">Evidence {index + 1}</span>
                </span>
                <span className="min-w-0 truncate text-xs text-muted-foreground">
                  {sourceLabel} · {shortEvidenceRef(citation.evidenceRef)}
                </span>
              </summary>
              {citation.selectionReason ? (
                <p className="mt-2 text-xs leading-5 text-muted-foreground">
                  {citation.selectionReason}
                </p>
              ) : null}
              <blockquote className="mt-2 max-h-56 overflow-y-auto whitespace-pre-wrap break-words border-l-2 border-border pl-3 text-xs leading-5 text-muted-foreground">
                {citation.quotedText}
              </blockquote>
            </details>
          );
        })}
      </div>
    </section>
  );
}

function formatAssistantDisplayText(text: string, citations: AgentChatCitation[]): string {
  if (!text) {
    return "";
  }
  const citationIndex = new Map(
    citations.map((citation, index) => [citation.evidenceRef, String(index + 1)]),
  );
  const withoutRawRefs = text.replace(/\[(ev-[^\]]+)\]/g, (_match, evidenceRef: string) => {
    const marker = citationIndex.get(evidenceRef);
    return marker ? ` [${marker}]` : "";
  });
  return compactCitationMarkers(withoutRawRefs)
    .replace(/[ \t]+([,.;:!?])/g, "$1")
    .replace(/[ \t]{2,}/g, " ")
    .trim();
}

function ensureCitationMarkers(text: string, citations: AgentChatCitation[]): string {
  if (!text || citations.length === 0 || /\[\d+\]/.test(text)) {
    return text;
  }
  const markers = citations.slice(0, 4).map((_citation, index) => `[${index + 1}]`).join(" ");
  return `${text} ${markers}`;
}

function linkifyCitationMarkers(text: string, citations: AgentChatCitation[]): string {
  if (!text || citations.length === 0) {
    return text;
  }
  return text.replace(/\[(\d+)\](?!\()/g, (match, marker: string) => {
    const index = Number(marker);
    if (!Number.isInteger(index) || index < 1 || index > citations.length) {
      return match;
    }
    return `[${marker}](#citation-${marker})`;
  });
}

function compactCitationMarkers(text: string): string {
  return text.replace(/(?:\s*\[(\d+)\]){2,}/g, (match) => {
    const markers = [...match.matchAll(/\[(\d+)\]/g)].map((item) => item[1]);
    const uniqueMarkers = [...new Set(markers)];
    return uniqueMarkers.map((marker) => ` [${marker}]`).join("");
  });
}

function shortEvidenceRef(evidenceRef: string): string {
  const compactRef = evidenceRef.replace(/^ev-source-/, "ev-");
  return compactRef.length > 18 ? `${compactRef.slice(0, 18)}...` : compactRef;
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
      resultsByMessageId: parsed.resultsByMessageId ?? {},
    };
  } catch {
    return emptyStorage();
  }
}

function persistThreads(value: StoredThreads) {
  window.localStorage.setItem(STORAGE_KEY, JSON.stringify(value));
}

function emptyStorage(): StoredThreads {
  return { version: STORAGE_VERSION, threads: [], activeThreadId: null, resultsByMessageId: {} };
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

function updateThreadMessage(
  threads: AgentThread[],
  threadId: string,
  messageId: string,
  updater: (message: AgentChatMessage) => AgentChatMessage,
): AgentThread[] {
  return threads.map((thread) => {
    if (thread.id !== threadId) {
      return thread;
    }
    return {
      ...thread,
      updatedAt: unixTimestamp(),
      messages: thread.messages.map((message) =>
        message.id === messageId ? updater(message) : message,
      ),
    };
  });
}

function removeKey<T>(record: Record<string, T>, key: string): Record<string, T> {
  if (!(key in record)) {
    return record;
  }
  const next = { ...record };
  delete next[key];
  return next;
}

function statusLabel(status: AgentChatStreamStatus, fallback: string): string {
  switch (status) {
    case "resolving_scope":
      return "Preparing...";
    case "retrieving_context":
      return "Retrieving context...";
    case "classifying_question":
      return "Classifying question...";
    case "connecting_provider":
      return "Connecting provider...";
    case "generating":
      return "Generating...";
    case "validating_citations":
      return "Validating citations...";
    case "complete":
      return "Complete";
    default:
      return fallback;
  }
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
