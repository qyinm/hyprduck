import { useEffect, useMemo, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTermTerminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import { Plus, RotateCcw, Terminal as TerminalIcon, X } from "lucide-react";

import type {
  AgentTerminalAgent,
  AgentTerminalEvent,
  AgentTerminalListResult,
  AgentTerminalSession,
  DesktopUnlisten,
} from "@/appTypes";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

interface AgentTerminalProps {
  nodeId: string | null;
  onClose: () => void;
  onCreateSession: (args: {
    agentId: AgentTerminalAgent["id"];
    nodeId: string | null;
  }) => Promise<AgentTerminalSession>;
  onListenAgentTerminalEvents: (
    handler: (event: AgentTerminalEvent) => void,
  ) => DesktopUnlisten;
  onListAgents: () => Promise<AgentTerminalListResult>;
  onKillSession: (args: { sessionId: string }) => Promise<unknown>;
  onResizeSession: (args: {
    sessionId: string;
    cols: number;
    rows: number;
  }) => Promise<unknown>;
  onWriteSession: (args: {
    sessionId: string;
    input: string;
  }) => Promise<unknown>;
  open: boolean;
}

export function AgentTerminal(props: AgentTerminalProps) {
  const {
    nodeId,
    onClose,
    onCreateSession,
    onListenAgentTerminalEvents,
    onListAgents,
    onKillSession,
    onResizeSession,
    onWriteSession,
    open,
  } = props;
  const terminalHostRef = useRef<HTMLDivElement | null>(null);
  const terminalRef = useRef<XTermTerminal | null>(null);
  const fitAddonRef = useRef<FitAddon | null>(null);
  const renderedSessionIdRef = useRef<string | null>(null);
  const renderedOutputLengthRef = useRef(0);
  const sessionsRef = useRef<AgentTerminalSession[]>([]);
  const onKillSessionRef = useRef(onKillSession);
  const onResizeSessionRef = useRef(onResizeSession);
  const onWriteSessionRef = useRef(onWriteSession);
  const [agentList, setAgentList] = useState<AgentTerminalListResult | null>(null);
  const [sessions, setSessions] = useState<AgentTerminalSession[]>([]);
  const [activeSessionId, setActiveSessionId] = useState<string | null>(null);
  const [pickerOpen, setPickerOpen] = useState(false);
  const [loadingAgents, setLoadingAgents] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const activeSession = useMemo(
    () => sessions.find((session) => session.id === activeSessionId) ?? null,
    [activeSessionId, sessions],
  );

  useEffect(() => {
    onKillSessionRef.current = onKillSession;
    onResizeSessionRef.current = onResizeSession;
    onWriteSessionRef.current = onWriteSession;
  }, [onKillSession, onResizeSession, onWriteSession]);

  useEffect(() => {
    sessionsRef.current = sessions;
  }, [sessions]);

  useEffect(
    () => () => {
      for (const session of sessionsRef.current.filter(
        (candidate) => candidate.status !== "closed",
      )) {
        void onKillSessionRef.current({ sessionId: session.id }).catch(
          () => undefined,
        );
      }
    },
    [],
  );

  useEffect(() => {
    if (!open || agentList || loadingAgents) {
      return;
    }
    void refreshAgents();
  }, [agentList, loadingAgents, open]);

  useEffect(() => {
    if (!open) {
      return undefined;
    }
    return onListenAgentTerminalEvents((event) => {
      setSessions((current) =>
        current.map((session) =>
          session.id === event.session.id ? event.session : session,
        ),
      );
    });
  }, [onListenAgentTerminalEvents, open]);

  useEffect(() => {
    if (open) {
      return undefined;
    }
    terminalRef.current?.dispose();
    terminalRef.current = null;
    fitAddonRef.current = null;
    renderedSessionIdRef.current = null;
    renderedOutputLengthRef.current = 0;
    const liveSessions = sessionsRef.current.filter(
      (session) => session.status !== "closed",
    );
    for (const session of liveSessions) {
      void onKillSessionRef.current({ sessionId: session.id }).catch(() => undefined);
    }
    setSessions([]);
    setActiveSessionId(null);
    return undefined;
  }, [open]);

  useEffect(() => {
    terminalRef.current?.dispose();
    terminalRef.current = null;
    fitAddonRef.current = null;
    renderedSessionIdRef.current = null;
    renderedOutputLengthRef.current = 0;

    if (
      !activeSession ||
      !terminalHostRef.current ||
      activeSession.status === "fallback_required"
    ) {
      return undefined;
    }

    const terminal = new XTermTerminal({
      cursorBlink: true,
      fontFamily:
        "ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace",
      fontSize: 13,
      macOptionIsMeta: true,
      scrollback: 10_000,
      theme: {
        background: "#09090b",
        foreground: "#f4f4f5",
        cursor: "#ffffff",
        selectionBackground: "#3f3f46",
      },
    });
    const fitAddon = new FitAddon();
    terminal.loadAddon(fitAddon);
    terminal.open(terminalHostRef.current);
    fitAddon.fit();
    terminal.focus();

    terminalRef.current = terminal;
    fitAddonRef.current = fitAddon;
    renderedSessionIdRef.current = activeSession.id;
    terminal.write(activeSession.output ?? "");
    renderedOutputLengthRef.current = activeSession.output?.length ?? 0;
    terminal.attachCustomKeyEventHandler((event) => {
      if (event.type !== "keydown") {
        return true;
      }
      const modifierPressed = isApplePlatform()
        ? event.metaKey
        : event.ctrlKey && !event.altKey;
      if (!modifierPressed) {
        return true;
      }
      const key = event.key.toLowerCase();
      if (key === "c" && terminal.hasSelection()) {
        void navigator.clipboard
          ?.writeText(terminal.getSelection())
          .catch(() => undefined);
        return false;
      }
      if (key === "v") {
        void navigator.clipboard
          ?.readText()
          .then((text) => {
            if (text) {
              return onWriteSessionRef.current({
                sessionId: activeSession.id,
                input: text,
              });
            }
            return undefined;
          })
          .catch((pasteError) => setError(String(pasteError)));
        return false;
      }
      if (key === "a") {
        terminal.selectAll();
        return false;
      }
      return true;
    });

    const dataSubscription = terminal.onData((input) => {
      void onWriteSessionRef.current({ sessionId: activeSession.id, input }).catch(
        (writeError) => setError(String(writeError)),
      );
    });
    let lastCols = terminal.cols;
    let lastRows = terminal.rows;
    void onResizeSessionRef.current({
      sessionId: activeSession.id,
      cols: terminal.cols,
      rows: terminal.rows,
    }).catch(() => undefined);
    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
      if (terminal.cols === lastCols && terminal.rows === lastRows) {
        return;
      }
      lastCols = terminal.cols;
      lastRows = terminal.rows;
      void onResizeSessionRef.current({
        sessionId: activeSession.id,
        cols: terminal.cols,
        rows: terminal.rows,
      }).catch(() => undefined);
    });
    resizeObserver.observe(terminalHostRef.current);

    return () => {
      resizeObserver.disconnect();
      dataSubscription.dispose();
      terminal.dispose();
      if (terminalRef.current === terminal) {
        terminalRef.current = null;
        fitAddonRef.current = null;
      }
    };
  }, [activeSession?.id, activeSession?.status]);

  useEffect(() => {
    if (
      !activeSession ||
      !terminalRef.current ||
      renderedSessionIdRef.current !== activeSession.id
    ) {
      return;
    }
    const output = activeSession.output ?? "";
    const renderedLength = renderedOutputLengthRef.current;
    if (output.length < renderedLength) {
      terminalRef.current.clear();
      terminalRef.current.write(output);
      renderedOutputLengthRef.current = output.length;
      return;
    }
    if (output.length > renderedLength) {
      terminalRef.current.write(output.slice(renderedLength));
      renderedOutputLengthRef.current = output.length;
    }
  }, [activeSession, activeSession?.output, activeSession?.outputSequence]);

  if (!open) {
    return null;
  }

  async function refreshAgents() {
    setLoadingAgents(true);
    setError(null);
    try {
      setAgentList(await onListAgents());
    } catch (refreshError) {
      setError(String(refreshError));
    } finally {
      setLoadingAgents(false);
    }
  }

  async function launchAgent(agent: AgentTerminalAgent) {
    if (!agent.detected) {
      return;
    }
    setError(null);
    try {
      const session = await onCreateSession({ agentId: agent.id, nodeId });
      setSessions((current) => [...current, session]);
      setActiveSessionId(session.id);
      setPickerOpen(false);
    } catch (launchError) {
      setError(String(launchError));
    }
  }

  async function closeSession(sessionId: string) {
    setSessions((current) => current.filter((session) => session.id !== sessionId));
    setActiveSessionId((current) => {
      if (current !== sessionId) {
        return current;
      }
      const remaining = sessionsRef.current.filter(
        (session) => session.id !== sessionId,
      );
      return remaining[remaining.length - 1]?.id ?? null;
    });
    try {
      await onKillSession({ sessionId });
    } catch (killError) {
      setError(String(killError));
    }
  }

  return (
    <div className="flex h-full min-h-0 w-full flex-col overflow-hidden">
      <header className="flex h-12 shrink-0 items-center justify-between gap-3 border-b border-border px-4">
        <div className="flex min-w-0 items-center gap-2">
          <TerminalIcon size={16} className="shrink-0 text-muted-foreground" />
          <h2 className="truncate text-sm font-semibold">Agent Terminal</h2>
        </div>
        <div className="flex items-center gap-1">
          <Button
            aria-label="Refresh agents"
            onClick={() => void refreshAgents()}
            size="icon"
            type="button"
            variant="ghost"
          >
            <RotateCcw size={14} />
          </Button>
          <Button
            aria-label="New agent tab"
            onClick={() => setPickerOpen((value) => !value)}
            size="icon"
            type="button"
            variant="ghost"
          >
            <Plus size={15} />
          </Button>
          <Button
            aria-label="Close Agent Terminal"
            onClick={onClose}
            size="icon"
            type="button"
            variant="ghost"
          >
            <X size={15} />
          </Button>
        </div>
      </header>

      {sessions.length > 0 ? (
        <div className="flex h-10 shrink-0 items-center gap-1 overflow-x-auto border-b border-border bg-secondary/20 px-3">
          {sessions.map((session) => (
            <div
              className={cn(
                "flex h-7 max-w-48 shrink-0 items-center gap-1 rounded-md border pl-2 pr-1 text-xs font-medium",
                session.id === activeSessionId
                  ? "border-foreground bg-background text-foreground"
                  : "border-transparent text-muted-foreground hover:bg-background",
              )}
              key={session.id}
            >
              <button
                className="min-w-0 flex-1 truncate text-left"
                onClick={() => setActiveSessionId(session.id)}
                type="button"
              >
                {session.agent.label}
              </button>
              <button
                aria-label={`Close ${session.agent.label} tab`}
                className="grid size-5 shrink-0 place-items-center rounded text-muted-foreground hover:bg-secondary hover:text-foreground"
                onClick={() => void closeSession(session.id)}
                type="button"
              >
                <X size={11} />
              </button>
            </div>
          ))}
        </div>
      ) : null}

      <div className="relative min-h-0 flex-1 bg-zinc-950 p-4 font-mono text-xs text-zinc-100">
        {activeSession ? (
          activeSession.status === "fallback_required" ? (
            <pre className="h-full whitespace-pre-wrap leading-5 text-zinc-300">
              {activeSession.backend.reason ??
                "Native backend is not available for this session."}
            </pre>
          ) : (
            <div className="h-full min-h-0 overflow-hidden" ref={terminalHostRef} />
          )
        ) : (
          <div className="flex h-full items-center justify-center">
            <div className="grid w-full max-w-sm gap-2">
              {(agentList?.agents ?? [])
                .filter((agent) => agent.detected)
                .slice(0, 4)
                .map((agent) => (
                  <Button
                    className="justify-between border-zinc-700 bg-zinc-900 text-zinc-100 hover:bg-zinc-800 hover:text-zinc-50"
                    key={agent.id}
                    onClick={() => void launchAgent(agent)}
                    type="button"
                    variant="outline"
                  >
                    <span className="truncate">{agent.label}</span>
                    <Badge variant="outline" className="border-zinc-700 text-zinc-300">
                      {agent.command}
                    </Badge>
                  </Button>
                ))}
              {loadingAgents ? (
                <span className="text-center text-xs text-zinc-500">Loading</span>
              ) : null}
            </div>
          </div>
        )}

        {pickerOpen ? (
          <div className="absolute left-3 top-3 w-72 rounded-lg border border-border bg-background p-2 shadow-lg">
            <div className="mb-2 flex items-center justify-between gap-2">
              <span className="text-xs font-semibold">Detected agents</span>
              {loadingAgents ? (
                <span className="text-[10px] text-muted-foreground">Loading</span>
              ) : null}
            </div>
            <div className="grid gap-1">
              {(agentList?.agents ?? []).map((agent) => (
                <Button
                  className="h-auto justify-between gap-3 px-2 py-2 text-left"
                  disabled={!agent.detected}
                  key={agent.id}
                  onClick={() => void launchAgent(agent)}
                  type="button"
                  variant="ghost"
                >
                  <span className="min-w-0">
                    <span className="block truncate text-xs font-medium">
                      {agent.label}
                    </span>
                    <span className="block truncate text-[10px] text-muted-foreground">
                      {agent.path ?? agent.disabledReason}
                    </span>
                  </span>
                  <Badge variant={agent.detected ? "default" : "outline"}>
                    {agent.detected ? agent.support : "missing"}
                  </Badge>
                </Button>
              ))}
            </div>
            <p className="mt-2 border-t border-border pt-2 text-[10px] leading-4 text-muted-foreground">
              {agentList?.shell.reason ??
                "Generic shell/custom commands are disabled in v1."}
            </p>
          </div>
        ) : null}
      </div>

      {error ? (
        <p className="border-t border-border px-3 py-2 text-xs text-destructive">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function isApplePlatform() {
  return /Mac|iPhone|iPad|iPod/i.test(navigator.platform);
}
