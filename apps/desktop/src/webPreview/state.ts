import type {
  AgentChatStreamEvent,
  DesktopMessage,
  EngineConfigPayload,
  UiSnapshot,
  ValidateProviderResponseData,
} from "@/appTypes";

import { WEB_MOCK_BASE_SNAPSHOT, WEB_MOCK_CONFIG } from "./fixtures";

export let webMockSnapshot = WEB_MOCK_BASE_SNAPSHOT;
export let webMockConfig: EngineConfigPayload = WEB_MOCK_CONFIG;
export let webMockValidation: ValidateProviderResponseData = { ready: false, issues: [] };
export let webMockParseTimer: ReturnType<typeof setTimeout> | null = null;

export const webMockSnapshotListeners = new Set<
  (message: DesktopMessage<UiSnapshot>) => void
>();
export const webMockAgentChatListeners = new Set<
  (message: DesktopMessage<AgentChatStreamEvent>) => void
>();
export const webMockAgentChatTimers = new Map<string, ReturnType<typeof setTimeout>[]>();

export function setWebMockSnapshot(snapshot: UiSnapshot) {
  webMockSnapshot = snapshot;
}

export function setWebMockConfig(config: EngineConfigPayload) {
  webMockConfig = config;
}

export function setWebMockValidation(validation: ValidateProviderResponseData) {
  webMockValidation = validation;
}

export function setWebMockParseTimer(timer: ReturnType<typeof setTimeout> | null) {
  webMockParseTimer = timer;
}

export function emitWebSnapshot(snapshot: UiSnapshot) {
  webMockSnapshot = snapshot;
  const payload: DesktopMessage<UiSnapshot> = { payload: snapshot };
  for (const listener of webMockSnapshotListeners) {
    void Promise.resolve()
      .then(() => listener(payload))
      .catch((error: unknown) => {
        console.error("Web mock listener error:", error);
      });
  }
}

export function emitWebAgentChatEvent(event: AgentChatStreamEvent) {
  const payload: DesktopMessage<AgentChatStreamEvent> = { payload: event };
  for (const listener of webMockAgentChatListeners) {
    void Promise.resolve()
      .then(() => listener(payload))
      .catch((error: unknown) => {
        console.error("Web mock agent chat listener error:", error);
      });
  }
}
