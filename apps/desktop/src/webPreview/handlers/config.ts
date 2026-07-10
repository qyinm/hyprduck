import type {
  EngineConfigPayload,
  RuntimeReadinessCheck,
  RuntimeReadinessResponseData,
  ValidateProviderResponseData,
  ValidationIssue,
} from "@/appTypes";

import {
  WEB_MOCK_PROVIDER_MODELS,
  WEB_MOCK_PROVIDER_OPTIONS,
} from "../fixtures";
import {
  setWebMockConfig,
  setWebMockValidation,
  webMockConfig,
} from "../state";

export function deriveWebValidation(
  payload: EngineConfigPayload | null,
): ValidateProviderResponseData {
  const config = payload ?? webMockConfig;
  const provider = WEB_MOCK_PROVIDER_OPTIONS.find(
    (option) => option.id === config.provider,
  );
  const issues: ValidationIssue[] = [];
  if (provider?.requires_api_key && !config.api_key.trim()) {
    issues.push({
      code: "provider_config",
      message: `${provider.label} requires an API key.`,
    });
  }
  return {
    ready: issues.length === 0,
    issues,
  };
}

export function deriveWebReadiness(): RuntimeReadinessResponseData {
  const validation = deriveWebValidation(webMockConfig);
  const checks: RuntimeReadinessCheck[] = [
    {
      id: "runtime_process",
      label: "Runtime process",
      ready: false,
      required: true,
      message: "Desktop runtime is not available in web preview mode.",
    },
    {
      id: "config_file",
      label: "Engine config",
      ready: true,
      required: true,
      message: "Preview configuration is loaded in memory.",
    },
    {
      id: "provider_config",
      label: "Provider config",
      ready: validation.ready,
      required: true,
      message: validation.ready
        ? `${webMockConfig.provider} is configured for preview.`
        : validation.issues.map((issue) => issue.message).join(" "),
    },
  ];
  return {
    ready: checks
      .filter((check) => check.required)
      .every((check) => check.ready),
    provider: webMockConfig.provider,
    model_id: webMockConfig.model_id,
    checks,
  };
}

export const configHandlers = {
  load_engine_config: () => ({
    ...webMockConfig,
    provider_options: [...WEB_MOCK_PROVIDER_OPTIONS],
  }),
  validate_engine_config: (args: { payload?: EngineConfigPayload | null } | undefined) => {
    const next = deriveWebValidation(args?.payload ?? null);
    setWebMockValidation(next);
    return { ...next };
  },
  engine_readiness: () => deriveWebReadiness(),
  get_models_for_provider: (args: { providerSlug: string }) => {
    const key = args.providerSlug ?? webMockConfig.provider ?? "ollama";
    return [...(WEB_MOCK_PROVIDER_MODELS[key] ?? WEB_MOCK_PROVIDER_MODELS.ollama)];
  },
  save_engine_config: (args: { payload: EngineConfigPayload }) => {
    const next = {
      ...webMockConfig,
      ...args.payload,
      provider_options: WEB_MOCK_PROVIDER_OPTIONS,
    };
    setWebMockConfig(next);
    return { ...next };
  },
};
