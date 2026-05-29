import { useEffect, useMemo, useState } from "react";
import { ExternalLink, Plus, RotateCcw, Terminal, X } from "lucide-react";

import type {
  AgentTerminalAgent,
  AgentTerminalListResult,
  AgentTerminalSession,
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
  onListAgents: () => Promise<AgentTerminalListResult>;
  open: boolean;
  projectId: string | null;
  workspaceId: string;
}

export function AgentTerminal(props: AgentTerminalProps) {
  const {
    nodeId,
    onClose,
    onCreateSession,
    onListAgents,
    open,
    projectId,
    workspaceId,
  } = props;
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
    if (!open || agentList || loadingAgents) {
      return;
    }
    void refreshAgents();
  }, [agentList, loadingAgents, open]);

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

  return (
    <section className="pointer-events-auto absolute inset-x-6 bottom-24 z-30 mx-auto flex h-[min(34rem,calc(100%-8rem))] w-[min(64rem,calc(100%-3rem))] flex-col overflow-hidden rounded-lg border border-border bg-background shadow-[0_24px_80px_rgba(15,23,42,0.18)]">
      <header className="flex h-12 shrink-0 items-center justify-between gap-3 border-b border-border px-3">
        <div className="flex min-w-0 items-center gap-2">
          <Terminal size={16} className="shrink-0 text-muted-foreground" />
          <h2 className="truncate text-sm font-semibold">Agent Terminal</h2>
          <Badge variant="outline" className="text-[10px]">
            {workspaceId}
          </Badge>
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

      <div className="flex h-10 shrink-0 items-center gap-1 overflow-x-auto border-b border-border bg-secondary/20 px-2">
        {sessions.map((session) => (
          <button
            className={cn(
              "h-7 max-w-48 shrink-0 rounded-md border px-2 text-xs font-medium",
              session.id === activeSessionId
                ? "border-foreground bg-background text-foreground"
                : "border-transparent text-muted-foreground hover:bg-background",
            )}
            key={session.id}
            onClick={() => setActiveSessionId(session.id)}
            type="button"
          >
            <span className="truncate">{session.agent.label}</span>
          </button>
        ))}
        {sessions.length === 0 ? (
          <span className="px-2 text-xs text-muted-foreground">No agent tabs</span>
        ) : null}
      </div>

      <div className="relative grid min-h-0 flex-1 grid-cols-[minmax(0,1fr)_18rem]">
        <div className="min-h-0 bg-zinc-950 p-4 font-mono text-xs text-zinc-100">
          {activeSession ? (
            <div className="flex h-full flex-col gap-3">
              <div className="flex items-center justify-between gap-3 border-b border-zinc-800 pb-3">
                <span className="truncate">{activeSession.agent.command}</span>
                <span className="rounded border border-amber-400/30 px-2 py-1 text-[10px] text-amber-200">
                  {activeSession.status}
                </span>
              </div>
              <pre className="whitespace-pre-wrap leading-5 text-zinc-300">
                {activeSession.backend.reason ??
                  "Native backend is ready to attach once the Ghostty spike passes."}
              </pre>
            </div>
          ) : (
            <div className="flex h-full items-center justify-center text-center text-zinc-400">
              Select a detected agent to open a tab.
            </div>
          )}
        </div>

        <aside className="min-h-0 overflow-y-auto border-l border-border bg-background p-3">
          <h3 className="text-xs font-semibold text-foreground">Context Handoff</h3>
          <dl className="mt-3 grid gap-2 text-xs">
            <div>
              <dt className="text-muted-foreground">Workspace</dt>
              <dd className="mt-0.5 truncate font-medium">{workspaceId}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">Project</dt>
              <dd className="mt-0.5 truncate font-medium">{projectId ?? "none"}</dd>
            </div>
            <div>
              <dt className="text-muted-foreground">MCP</dt>
              <dd className="mt-0.5 font-medium">
                {activeSession?.handoff.mcp.status ?? "available"}
              </dd>
            </div>
          </dl>
          <ol className="mt-3 grid gap-1.5 text-xs leading-5 text-muted-foreground">
            {(activeSession?.handoff.context.attachInstructions ?? [
              `Workspace: ${workspaceId}`,
              "Call HyprDuck MCP get_context_pack before answering.",
              "Use cited evidence refs from the returned context pack.",
            ]).map((instruction) => (
              <li key={instruction}>{instruction}</li>
            ))}
          </ol>
          <div className="mt-4 flex items-center gap-2 rounded-md border border-border bg-secondary/20 p-2 text-xs text-muted-foreground">
            <ExternalLink size={14} className="shrink-0" />
            External Ghostty fallback remains official until native parity passes.
          </div>
        </aside>

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
    </section>
  );
}
