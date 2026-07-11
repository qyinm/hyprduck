/**
 * FREEZE: Agent Terminal surface is maintenance-only.
 * Do not expand features, rewrite session UX, or couple new product flows here
 * without an explicit unfreeze decision. Keep behavior-preserving bugfixes only.
 * Backends/tests under main/agent-terminal-* remain; do not delete them.
 */
import { type ReactNode, useEffect, useMemo, useRef, useState } from "react";
import { FitAddon } from "@xterm/addon-fit";
import { Terminal as XTermTerminal } from "@xterm/xterm";
import "@xterm/xterm/css/xterm.css";
import {
  Minimize2,
  Plus,
  RotateCcw,
  Settings,
  SquareTerminal,
  Terminal as TerminalIcon,
  X,
} from "lucide-react";

import type {
  AgentTerminalAgent,
  AgentTerminalEvent,
  AgentTerminalListResult,
  AgentTerminalSession,
  DesktopUnlisten,
} from "@/appTypes";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import claudeIconUrl from "../../../resources/icons/claude-color.svg?url";
import hermesIconUrl from "../../../resources/icons/hermesagent.svg?url";
import openAiIconUrl from "../../../resources/icons/openai.svg?url";
import piAgentIconUrl from "../../../resources/icons/pi-agent.svg?url";

const TERMINAL_OUTPUT_LIMIT = 300_000;

interface AgentTerminalProps {
  nodeId: string | null;
  onClose: () => void;
  onMinimize: () => void;
  onCreateSession: (args: {
    kind?: "agent" | "shell";
    agentId?: AgentTerminalAgent["id"];
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
    onMinimize,
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
  const pickerContainerRef = useRef<HTMLDivElement | null>(null);
  const renderedSessionIdRef = useRef<string | null>(null);
  const renderedOutputLengthRef = useRef(0);
  const sessionsRef = useRef<AgentTerminalSession[]>([]);
  const pendingOutputEventsRef = useRef(
    new Map<string, { session: AgentTerminalSession; outputDelta: string }>(),
  );
  const pendingOutputFrameRef = useRef<number | null>(null);
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
    function flushOutputEvents() {
      pendingOutputFrameRef.current = null;
      const pendingEvents = Array.from(pendingOutputEventsRef.current.values());
      pendingOutputEventsRef.current.clear();
      if (pendingEvents.length === 0) {
        return;
      }
      setSessions((current) =>
        current.map((session) => {
          const pendingEvent = pendingEvents.find(
            (candidate) => candidate.session.id === session.id,
          );
          if (!pendingEvent) {
            return session;
          }
          return {
            ...session,
            ...pendingEvent.session,
            output: trimTerminalOutput(
              `${session.output ?? ""}${pendingEvent.outputDelta}`,
            ),
          };
        }),
      );
    }
    function scheduleOutputFlush(event: AgentTerminalEvent) {
      const current = pendingOutputEventsRef.current.get(event.session.id);
      pendingOutputEventsRef.current.set(event.session.id, {
        session: event.session,
        outputDelta: `${current?.outputDelta ?? ""}${event.outputDelta ?? ""}`,
      });
      if (pendingOutputFrameRef.current !== null) {
        return;
      }
      pendingOutputFrameRef.current = window.requestAnimationFrame(flushOutputEvents);
    }
    const unlisten = onListenAgentTerminalEvents((event) => {
      if (event.outputDelta !== undefined) {
        scheduleOutputFlush(event);
        return;
      }
      setSessions((current) =>
        current.map((session) =>
          session.id === event.session.id ? { ...session, ...event.session } : session,
        ),
      );
    });
    return () => {
      unlisten();
      if (pendingOutputFrameRef.current !== null) {
        window.cancelAnimationFrame(pendingOutputFrameRef.current);
        pendingOutputFrameRef.current = null;
      }
      pendingOutputEventsRef.current.clear();
    };
  }, [onListenAgentTerminalEvents, open]);

  useEffect(() => {
    if (!pickerOpen) {
      return undefined;
    }
    function closePickerOnOutsidePointer(event: PointerEvent) {
      const target = event.target;
      if (
        target instanceof Node &&
        pickerContainerRef.current?.contains(target)
      ) {
        return;
      }
      setPickerOpen(false);
    }
    document.addEventListener("pointerdown", closePickerOnOutsidePointer, true);
    return () => {
      document.removeEventListener(
        "pointerdown",
        closePickerOnOutsidePointer,
        true,
      );
    };
  }, [pickerOpen]);

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
    setPickerOpen(false);
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

  async function launchShell() {
    if (!agentList?.shell.available) {
      setError(agentList?.shell.reason ?? "No executable default shell was found.");
      return;
    }
    setError(null);
    try {
      const session = await onCreateSession({ kind: "shell", nodeId });
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
      <header className="flex h-12 shrink-0 items-center justify-between gap-3 border-b border-zinc-800 px-4 text-zinc-100">
        <div className="flex min-w-0 items-center gap-2">
          <TerminalIcon size={16} className="shrink-0 text-zinc-500" />
          <h2 className="truncate text-sm font-semibold">New Terminal</h2>
        </div>
        <div className="flex items-center gap-1">
          <Button
            aria-label="Minimize Agent Terminal"
            className="text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
            onClick={onMinimize}
            size="icon"
            type="button"
            variant="ghost"
          >
            <Minimize2 size={14} />
          </Button>
          <Button
            aria-label="Refresh agents"
            className="text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
            onClick={() => void refreshAgents()}
            size="icon"
            type="button"
            variant="ghost"
          >
            <RotateCcw size={14} />
          </Button>
          <Button
            aria-label="Close Agent Terminal"
            className="text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
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
        <div className="relative z-20 h-10 shrink-0 overflow-visible border-b border-zinc-800 bg-zinc-900">
          <div className="flex h-full items-center px-3">
            <div className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto">
              {sessions.map((session) => (
                <div
                  className={cn(
                    "flex h-7 max-w-48 shrink-0 items-center gap-1 rounded-md border pl-2 pr-1 text-xs font-medium",
                    session.id === activeSessionId
                      ? "border-zinc-600 bg-zinc-800 text-zinc-100"
                      : "border-transparent text-zinc-500 hover:bg-zinc-800 hover:text-zinc-200",
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
                    className="grid size-5 shrink-0 place-items-center rounded text-zinc-500 hover:bg-zinc-700 hover:text-zinc-100"
                    onClick={() => void closeSession(session.id)}
                    type="button"
                  >
                    <X size={11} />
                  </button>
                </div>
              ))}
            </div>
            <div className="relative ml-1 shrink-0" ref={pickerContainerRef}>
              <Button
                aria-label="New agent tab"
                className="size-7 text-zinc-400 hover:bg-zinc-800 hover:text-zinc-100"
                onClick={() => setPickerOpen((value) => !value)}
                size="icon"
                type="button"
                variant="ghost"
              >
                <Plus size={14} />
              </Button>
              {pickerOpen ? (
                <AgentTerminalPickerMenu
                  agents={agentList?.agents ?? []}
                  onLaunchAgent={launchAgent}
                  onLaunchShell={launchShell}
                  shellAvailable={agentList?.shell.available ?? false}
                  shellReason={agentList?.shell.reason ?? null}
                />
              ) : null}
            </div>
          </div>
        </div>
      ) : null}

      <div className="relative z-0 min-h-0 flex-1 bg-zinc-950/95 p-4 font-mono text-xs text-zinc-100">
        {activeSession ? (
          activeSession.status === "fallback_required" ? (
            <pre className="h-full whitespace-pre-wrap leading-5 text-zinc-300">
              {activeSession.backend.reason ??
                "Native backend is not available for this session."}
            </pre>
          ) : (
            <div className="h-full min-h-0 overflow-hidden" ref={terminalHostRef} />
          )
        ) : pickerOpen ? (
          <div className="h-full" />
        ) : (
          <div className="flex h-full items-center justify-center">
            <div className="grid w-full max-w-sm gap-2">
              {agentList?.shell.available ? (
                <Button
                  className="h-11 justify-start gap-3 border-zinc-700 bg-zinc-900 px-3 text-left font-sans text-sm font-semibold text-zinc-100 hover:bg-zinc-800 hover:text-zinc-50"
                  onClick={() => void launchShell()}
                  title={agentList.shell.path ?? undefined}
                  type="button"
                  variant="outline"
                >
                  <SquareTerminal size={18} className="shrink-0 text-zinc-300" />
                  <span className="min-w-0 flex-1 truncate">New Terminal</span>
                </Button>
              ) : null}
              {(agentList?.agents ?? [])
                .filter((agent) => agent.detected)
                .slice(0, 4)
                .map((agent) => (
                  <Button
                    className="h-11 justify-start gap-3 border-zinc-700 bg-zinc-900 px-3 text-left font-sans text-sm font-semibold text-zinc-100 hover:bg-zinc-800 hover:text-zinc-50"
                    key={agent.id}
                    onClick={() => void launchAgent(agent)}
                    title={agent.path ?? undefined}
                    type="button"
                    variant="outline"
                  >
                    <AgentMenuIcon agentId={agent.id} />
                    <span className="min-w-0 flex-1 truncate">{agent.label}</span>
                  </Button>
                ))}
              {loadingAgents ? (
                <span className="text-center text-xs text-zinc-500">Loading</span>
              ) : null}
            </div>
          </div>
        )}

      </div>

      {error ? (
        <p className="border-t border-zinc-800 px-3 py-2 text-xs text-red-300">
          {error}
        </p>
      ) : null}
    </div>
  );
}

function isApplePlatform() {
  return /Mac|iPhone|iPad|iPod/i.test(navigator.platform);
}

function trimTerminalOutput(output: string) {
  if (output.length <= TERMINAL_OUTPUT_LIMIT) {
    return output;
  }
  return output.slice(-TERMINAL_OUTPUT_LIMIT);
}

function TerminalMenuAction(props: {
  disabled?: boolean;
  icon: ReactNode;
  label: string;
  onClick?: () => void;
  shortcut: string;
}) {
  const { disabled = false, icon, label, onClick, shortcut } = props;
  return (
    <button
      className={cn(
        "flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[11px] font-semibold transition",
        disabled
          ? "cursor-not-allowed text-zinc-600"
          : "text-zinc-100 hover:bg-zinc-800",
      )}
      disabled={disabled}
      onClick={onClick}
      type="button"
    >
      <span className="grid size-3.5 shrink-0 place-items-center text-zinc-300">
        {icon}
      </span>
      <span className="min-w-0 flex-1 truncate">{label}</span>
      <span className="shrink-0 text-[10px] font-medium text-zinc-500">
        {shortcut}
      </span>
    </button>
  );
}

function AgentTerminalPickerMenu(props: {
  agents: AgentTerminalAgent[];
  onLaunchAgent: (agent: AgentTerminalAgent) => void | Promise<void>;
  onLaunchShell: () => void | Promise<void>;
  shellAvailable: boolean;
  shellReason: string | null;
}) {
  const { agents, onLaunchAgent, onLaunchShell, shellAvailable, shellReason } =
    props;
  return (
    <div className="absolute right-0 top-8 z-50 w-48 rounded-lg border border-zinc-700/90 bg-zinc-950/95 p-1 font-sans shadow-[0_14px_36px_rgba(0,0,0,0.42)]">
      <div className="grid gap-0.5">
        <TerminalMenuAction
          disabled={!shellAvailable}
          icon={<SquareTerminal size={14} />}
          label="New Terminal"
          onClick={() => void onLaunchShell()}
          shortcut="⌘T"
        />
        {!shellAvailable && shellReason ? (
          <span className="px-2 pb-1 text-[10px] text-zinc-600">
            {shellReason}
          </span>
        ) : null}
      </div>
      <div className="my-1 h-px bg-zinc-800" />
      <div className="grid gap-0.5">
        {agents.map((agent) => (
          <button
            className={cn(
              "flex h-8 w-full items-center gap-2 rounded-md px-2 text-left text-[11px] font-semibold transition",
              agent.detected
                ? "text-zinc-100 hover:bg-zinc-800"
                : "cursor-not-allowed text-zinc-600",
            )}
            disabled={!agent.detected}
            key={agent.id}
            onClick={() => void onLaunchAgent(agent)}
            title={agent.path ?? agent.disabledReason ?? undefined}
            type="button"
          >
            <AgentMenuIcon agentId={agent.id} />
            <span className="min-w-0 flex-1 truncate">{agent.label}</span>
          </button>
        ))}
      </div>
      <div className="mt-1 border-t border-zinc-800 pt-1">
        <button
          className="flex h-7 w-full items-center gap-2 rounded-md px-2 text-left text-[11px] font-semibold text-zinc-500"
          disabled
          type="button"
        >
          <Settings size={14} />
          <span>Agent settings...</span>
        </button>
      </div>
    </div>
  );
}

function AgentMenuIcon(props: { agentId: AgentTerminalAgent["id"] }) {
  const { agentId } = props;
  const iconClass = "size-5 shrink-0 object-contain";
  switch (agentId) {
    case "codex":
      return (
        <img
          alt=""
          className={cn(iconClass, "brightness-0 invert opacity-90")}
          src={openAiIconUrl}
        />
      );
    case "claude_code":
      return <img alt="" className={iconClass} src={claudeIconUrl} />;
    case "pi_agent":
      return <img alt="" className={iconClass} src={piAgentIconUrl} />;
    case "hermes":
      return (
        <img
          alt=""
          className={cn(iconClass, "brightness-0 invert opacity-90")}
          src={hermesIconUrl}
        />
      );
    default:
      return <SquareTerminal size={18} className="shrink-0 text-zinc-400" />;
  }
}
