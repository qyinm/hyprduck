import type {
  AgentTerminalListResult,
  EngineConfigPayload,
  FileSelection,
  ProviderOption,
  UiSnapshot,
} from "@/appTypes";

export const WEB_MOCK_PROVIDER_OPTIONS: ProviderOption[] = [
  {
    id: "open_router",
    label: "OpenRouter",
    requires_api_key: true,
    supports_base_url: true,
  },
  {
    id: "ollama",
    label: "Ollama",
    requires_api_key: false,
    supports_base_url: true,
  },
];

export const WEB_MOCK_CONFIG: EngineConfigPayload = {
  provider: "ollama",
  model_id: "llama3.1",
  api_key: "",
  base_url: "http://localhost:11434",
  prompt_template: "General",
  provider_options: WEB_MOCK_PROVIDER_OPTIONS,
  model_options: ["llama3.1", "llava:latest", "qwen2.5vl"],
  prompt_template_options: [
    "General",
    "Tutorial",
    "UI flow",
    "Code",
    "Table",
  ],
};

export const WEB_MOCK_SAMPLE_FILE: FileSelection = {
  path: "/tmp/hyprduck-sample.pdf",
  format: "pdf",
};

export const WEB_MOCK_MARKDOWN = `# Sample import

## Page 1
This is a demonstration preview run in the browser.

## Page 2
The real Electron runtime is not available in this preview, so we show read-only sample behavior.
`;

export const WEB_MOCK_BASE_SNAPSHOT: UiSnapshot = {
  activeJob: null,
  progressLog: [
    {
      phase: "import",
      message: "Desktop runtime not detected. Running in browser preview mode.",
      timestamp: new Date().toISOString(),
    },
  ],
  lastResult: {
    savedOutputPath: "~/Library/Application Support/HyprDuck/web-preview/sample.md",
    successCount: 2,
    failedCount: 0,
    markdown: WEB_MOCK_MARKDOWN,
  },
  lastProjectId: "preview:sample",
  workspaceRevision: 0,
};

export const WEB_MOCK_PROVIDER_MODELS: Record<string, string[]> = {
  open_router: ["gpt-4o", "claude-3.5-sonnet", "llama-3.1-70b"],
  ollama: ["llama3.1", "llava:latest", "qwen2.5vl"],
};

export const WEB_MOCK_NOW_SECONDS = Math.floor(Date.now() / 1000);

/** Agent terminal is frozen / non-demo in web preview — list remains for surface presence only. */
export const WEB_MOCK_AGENT_LIST: AgentTerminalListResult = {
  agents: [
    {
      id: "codex",
      label: "Codex",
      detected: true,
      support: "supported",
      commands: ["codex"],
      command: "codex",
      path: "/usr/local/bin/codex",
      launchArgs: [],
      confidence: "high",
      disabledReason: null,
    },
    {
      id: "claude_code",
      label: "Claude Code",
      detected: false,
      support: "supported",
      commands: ["claude"],
      command: null,
      path: null,
      launchArgs: [],
      confidence: "missing",
      disabledReason: "Claude Code command was not found on PATH.",
    },
    {
      id: "pi_agent",
      label: "Pi Agent",
      detected: false,
      support: "experimental",
      commands: ["pi-agent"],
      command: null,
      path: null,
      launchArgs: [],
      confidence: "missing",
      disabledReason: "Pi Agent command was not found on PATH.",
    },
    {
      id: "hermes",
      label: "Hermes",
      detected: false,
      support: "experimental",
      commands: ["hermes"],
      command: null,
      path: null,
      launchArgs: [],
      confidence: "missing",
      disabledReason: "Hermes command was not found on PATH.",
    },
  ],
  shell: {
    available: false,
    label: null,
    command: null,
    path: null,
    reason: "Web preview cannot host native terminal sessions.",
  },
};
