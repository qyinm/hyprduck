import { useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";

import type {
  EngineConfigPayload,
  RuntimeReadinessResponseData,
  ValidateProviderResponseData,
} from "@/appTypes";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { cn } from "@/lib/utils";

export type SettingsTab = "general" | "ai";

const UI_LANGUAGE_STORAGE_KEY = "hyprduck.uiLanguage";
const UI_LANGUAGE_OPTIONS = [
  { id: "en", label: "English" },
  { id: "ko", label: "한국어" },
  { id: "ja", label: "日本語" },
] as const;

interface ProviderState {
  apiKey: string;
  baseUrl: string;
  expanded: boolean;
  showAdvanced: boolean;
}

function modelTaskGuidance(providerId: string, modelId: string) {
  const model = modelId.toLowerCase();

  if (providerId === "ollama") {
    if (
      model.includes("8b") ||
      model.includes("ocr") ||
      model.includes("llama3.1")
    ) {
      return {
        tone: "warning",
        title: "Local model caution",
        body: "This keeps data local, but small or OCR-only models can miss tables, conflicts, and evidence links. Run the golden corpus before relying on agent-ready outputs.",
      };
    }

    return {
      tone: "local",
      title: "Local-first path",
      body: "Good for private parsing and retrieval checks. Keep generated merge output disabled until the golden corpus is clean.",
    };
  }

  return {
    tone: "hosted",
    title: "Hosted quality path",
    body: "Recommended for high-recall page parsing, structured extraction, and merge verification when privacy policy allows hosted inference.",
  };
}

function settingsSignature(payload: {
  provider: string;
  model_id: string;
  api_key: string;
  base_url: string | null;
  prompt_template: string;
}) {
  return JSON.stringify({
    provider: payload.provider,
    model_id: payload.model_id,
    api_key: payload.api_key,
    base_url: payload.base_url ?? null,
    prompt_template: payload.prompt_template,
  });
}

function defaultProviderState(): ProviderState {
  return {
    apiKey: "",
    baseUrl: "",
    expanded: false,
    showAdvanced: false,
  };
}

export function SettingsPanel(props: {
  config: EngineConfigPayload | null;
  validation: ValidateProviderResponseData | null;
  readiness: RuntimeReadinessResponseData | null;
  onSave: (payload: EngineConfigPayload) => Promise<void>;
  onRefreshReadiness: () => Promise<void>;
  onLoadProviderModels: (providerId: string) => Promise<string[]>;
  tab: SettingsTab;
}) {
  const {
    config,
    validation,
    onSave,
    onRefreshReadiness,
    onLoadProviderModels,
    tab,
  } = props;
  const [promptTemplate, setPromptTemplate] = useState("General");
  const [uiLanguage, setUiLanguage] = useState("en");
  const [selectedModel, setSelectedModel] = useState("");
  const [activeProvider, setActiveProvider] = useState("open_router");
  const [providerStates, setProviderStates] = useState<
    Map<string, ProviderState>
  >(new Map());
  const [availableModels, setAvailableModels] = useState<string[]>([]);
  const lastSavedSettingsSignature = useRef<string | null>(null);

  useEffect(() => {
    const storedLanguage = window.localStorage.getItem(UI_LANGUAGE_STORAGE_KEY);
    if (
      storedLanguage &&
      UI_LANGUAGE_OPTIONS.some((option) => option.id === storedLanguage)
    ) {
      setUiLanguage(storedLanguage);
    }
  }, []);

  const handleUiLanguageChange = (language: string) => {
    setUiLanguage(language);
    window.localStorage.setItem(UI_LANGUAGE_STORAGE_KEY, language);
  };

  useEffect(() => {
    if (config) {
      setActiveProvider(config.provider);
      setSelectedModel(config.model_id);
      setPromptTemplate(config.prompt_template ?? "General");
      lastSavedSettingsSignature.current = settingsSignature(config);
      setProviderStates((prev) => {
        const next = new Map(prev);
        for (const opt of config.provider_options) {
          const existing = prev.get(opt.id);
          const isActive = opt.id === config.provider;
          next.set(opt.id, {
            apiKey: isActive ? config.api_key : existing?.apiKey ?? "",
            baseUrl: isActive ? config.base_url ?? "" : existing?.baseUrl ?? "",
            expanded: existing?.expanded ?? false,
            showAdvanced: existing?.showAdvanced ?? false,
          });
        }
        return next;
      });
    }
  }, [config]);

  const updateApiKey = (providerId: string, key: string) => {
    setProviderStates((prev) => {
      const next = new Map(prev);
      const existing = next.get(providerId) ?? defaultProviderState();
      next.set(providerId, { ...existing, apiKey: key });
      return next;
    });
  };

  const toggleExpanded = (providerId: string) => {
    setProviderStates((prev) => {
      const next = new Map(prev);
      const existing = next.get(providerId) ?? defaultProviderState();
      next.set(providerId, { ...existing, expanded: !existing.expanded });
      return next;
    });
  };

  const updateBaseUrl = (providerId: string, url: string) => {
    setProviderStates((prev) => {
      const next = new Map(prev);
      const existing = next.get(providerId) ?? defaultProviderState();
      next.set(providerId, { ...existing, baseUrl: url });
      return next;
    });
  };

  const toggleAdvanced = (providerId: string) => {
    setProviderStates((prev) => {
      const next = new Map(prev);
      const existing = next.get(providerId) ?? defaultProviderState();
      next.set(providerId, { ...existing, showAdvanced: !existing.showAdvanced });
      return next;
    });
  };

  const handleProviderChange = async (providerId: string) => {
    setActiveProvider(providerId);
    const models = await onLoadProviderModels(providerId);
    if (models.length > 0) {
      setSelectedModel(models[0]);
    }
  };

  useEffect(() => {
    if (activeProvider) {
      onLoadProviderModels(activeProvider)
        .then((models) => setAvailableModels(models))
        .catch(() => setAvailableModels([]));
    }
  }, [activeProvider, onLoadProviderModels]);

  const activeApiKey = providerStates.get(activeProvider)?.apiKey ?? "";
  const activeBaseUrl = providerStates.get(activeProvider)?.baseUrl ?? "";

  useEffect(() => {
    if (!config) return;
    const timer = setTimeout(() => {
      const activeState = providerStates.get(activeProvider);
      const payload: EngineConfigPayload = {
        provider: activeProvider,
        model_id: selectedModel,
        api_key: activeApiKey,
        base_url: activeState?.baseUrl || null,
        prompt_template: promptTemplate,
        provider_options: config?.provider_options ?? [],
        model_options: availableModels,
        prompt_template_options: config?.prompt_template_options ?? [],
      };
      const nextSignature = settingsSignature(payload);
      if (nextSignature === lastSavedSettingsSignature.current) {
        return;
      }
      lastSavedSettingsSignature.current = nextSignature;
      onSave(payload).catch(() => {
        lastSavedSettingsSignature.current = null;
      });
    }, 600);
    return () => clearTimeout(timer);
  }, [
    activeProvider,
    selectedModel,
    activeApiKey,
    activeBaseUrl,
    promptTemplate,
    availableModels,
    config,
    onSave,
    providerStates,
  ]);

  if (!config) {
    return (
      <div>
        <h2 className="text-base font-semibold mb-1">Settings</h2>
        <p className="text-sm text-muted-foreground">
          Loading engine configuration...
        </p>
      </div>
    );
  }

  const modelGuidance = modelTaskGuidance(activeProvider, selectedModel);

  return (
    <div className="space-y-8">
      {tab === "general" && (
        <section>
          <h2 className="text-base font-semibold mb-4">General</h2>
          <div className="max-w-sm space-y-2">
            <Label htmlFor="ui-language-select">UI language</Label>
            <select
              id="ui-language-select"
              className="h-9 w-full rounded-md border border-input bg-background px-3"
              value={uiLanguage}
              onChange={(e) => handleUiLanguageChange(e.target.value)}
            >
              {UI_LANGUAGE_OPTIONS.map((option) => (
                <option key={option.id} value={option.id}>
                  {option.label}
                </option>
              ))}
            </select>
          </div>
        </section>
      )}

      {tab === "ai" && (
        <section className="space-y-8">
          {validation && validation.issues.length > 0 && (
            <div className="rounded-lg border border-destructive/30 bg-destructive/5 p-3 text-xs leading-5 text-destructive">
              {validation.issues.map((issue) => (
                <p key={issue.code}>{issue.message}</p>
              ))}
            </div>
          )}

          <div>
            <div className="mb-4 flex items-start justify-between gap-4">
              <div>
                <h2 className="text-base font-semibold mb-1">AI model</h2>
                <p className="text-sm text-muted-foreground">
                  Choose how HyprDuck extracts document evidence.
                </p>
              </div>
              <Button
                className="h-8 shrink-0 px-3 text-xs"
                onClick={() => void onRefreshReadiness()}
                size="sm"
                type="button"
                variant="outline"
              >
                Refresh
              </Button>
            </div>
            <div className="grid gap-4 grid-cols-2">
              <div className="space-y-2">
                <Label htmlFor="active-provider">Provider</Label>
                <select
                  id="active-provider"
                  className="h-9 w-full rounded-md border border-input bg-background px-3"
                  value={activeProvider}
                  onChange={(e) => void handleProviderChange(e.target.value)}
                >
                  {config.provider_options.map((opt) => (
                    <option key={opt.id} value={opt.id}>
                      {opt.label}
                    </option>
                  ))}
                </select>
              </div>
              <div className="space-y-2">
                <Label htmlFor="active-model">Model</Label>
                <select
                  id="active-model"
                  className="h-9 w-full rounded-md border border-input bg-background px-3"
                  value={selectedModel}
                  onChange={(e) => setSelectedModel(e.target.value)}
                >
                  {availableModels.map((model) => (
                    <option key={model} value={model}>
                      {model}
                    </option>
                  ))}
                </select>
              </div>
            </div>
            <div
              className={cn(
                "mt-3 rounded-lg border px-3 py-2 text-xs leading-5",
                modelGuidance.tone === "warning"
                  ? "border-amber-200 bg-amber-50 text-amber-900"
                  : "border-border bg-secondary/50 text-muted-foreground",
              )}
            >
              <div className="font-medium text-foreground">
                {modelGuidance.title}
              </div>
              <p>{modelGuidance.body}</p>
            </div>
          </div>

          <div>
            <h2 className="text-base font-semibold mb-4">Connections</h2>
            <div className="space-y-2">
              {config.provider_options.map((opt) => {
                const state =
                  providerStates.get(opt.id) ?? defaultProviderState();
                return (
                  <div
                    key={opt.id}
                    className="rounded-lg border bg-card text-card-foreground"
                  >
                    <div
                      className="flex cursor-pointer items-center justify-between px-3 h-10"
                      onClick={() => toggleExpanded(opt.id)}
                      role="button"
                      tabIndex={0}
                      onKeyDown={(e) => {
                        if (e.key === "Enter" || e.key === " ") {
                          toggleExpanded(opt.id);
                        }
                      }}
                    >
                      <div className="flex items-center gap-2">
                        <span className="text-sm font-medium leading-none">
                          {opt.label}
                        </span>
                        {activeProvider === opt.id && (
                          <span className="rounded-full border border-border bg-secondary px-1.5 py-0 text-[10px] font-medium leading-none text-foreground">
                            Active
                          </span>
                        )}
                      </div>
                      {state.expanded ? (
                        <ChevronDown
                          size={12}
                          className="text-muted-foreground shrink-0"
                        />
                      ) : (
                        <ChevronRight
                          size={12}
                          className="text-muted-foreground shrink-0"
                        />
                      )}
                    </div>
                    {state.expanded && (
                      <div className="border-t px-3 py-2">
                        <div className="flex items-center gap-3">
                          <Label className="text-xs whitespace-nowrap leading-none text-muted-foreground shrink-0">
                            API Key
                          </Label>
                          <Input
                            autoComplete="off"
                            onChange={(e) =>
                              updateApiKey(opt.id, e.target.value)
                            }
                            placeholder={
                              opt.requires_api_key ? "Required" : "Optional"
                            }
                            type="password"
                            value={state.apiKey}
                            className="h-7 text-xs min-w-0"
                          />
                        </div>
                        {opt.supports_base_url && (
                          <>
                            <button
                              type="button"
                              onClick={() => toggleAdvanced(opt.id)}
                              className="mt-1.5 flex items-center gap-1 text-xs text-muted-foreground hover:text-foreground"
                            >
                              {state.showAdvanced ? (
                                <ChevronDown size={12} />
                              ) : (
                                <ChevronRight size={12} />
                              )}
                              Advanced
                            </button>
                            {state.showAdvanced && (
                              <div className="flex items-center gap-2 mt-1.5">
                                <Label className="text-xs whitespace-nowrap leading-none text-muted-foreground shrink-0">
                                  Base URL
                                </Label>
                                <Input
                                  autoComplete="off"
                                  onChange={(e) =>
                                    updateBaseUrl(opt.id, e.target.value)
                                  }
                                  placeholder={
                                    opt.id === "ollama"
                                      ? "http://localhost:11434"
                                      : opt.id === "open_router"
                                        ? "https://openrouter.ai/v1"
                                        : "Optional"
                                  }
                                  type="text"
                                  value={state.baseUrl}
                                  className="h-7 text-xs min-w-0"
                                />
                              </div>
                            )}
                          </>
                        )}
                      </div>
                    )}
                  </div>
                );
              })}
            </div>
          </div>
        </section>
      )}
    </div>
  );
}
