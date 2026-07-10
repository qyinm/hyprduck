import {
  type DesktopCommand,
  type DesktopCommandParameters,
  type DesktopCommandResult,
  type HyprDuckDesktopApi,
} from "@/appTypes";
import { createWebMockApi } from "@/webPreviewApi";

declare global {
  interface Window {
    hyprduck?: HyprDuckDesktopApi;
  }
}

const IS_WEB_PREVIEW = import.meta.env.VITE_PLATFORM === "web";

const webPreviewApi = IS_WEB_PREVIEW ? createWebMockApi() : null;

export function getDesktopApi(): HyprDuckDesktopApi {
  if (IS_WEB_PREVIEW) {
    return webPreviewApi as HyprDuckDesktopApi;
  }
  const api = window.hyprduck;
  if (!api) {
    throw new Error("HyprDuck desktop UI requires Electron preload APIs.");
  }
  return api;
}

export async function invoke<K extends DesktopCommand>(
  command: K,
  ...args: DesktopCommandParameters<K>
): Promise<DesktopCommandResult<K>> {
  return getDesktopApi().invoke(command, ...args);
}
