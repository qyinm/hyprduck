import {
  type DesktopCommand,
  type DesktopCommandParameters,
  type DesktopCommandResult,
  type EtymaDesktopApi,
} from "@/appTypes";
import { createWebMockApi } from "@/webPreviewApi";

declare global {
  interface Window {
    etyma?: EtymaDesktopApi;
  }
}

const IS_WEB_PREVIEW = import.meta.env.VITE_PLATFORM === "web";

const webPreviewApi = IS_WEB_PREVIEW ? createWebMockApi() : null;

export function getDesktopApi(): EtymaDesktopApi {
  if (IS_WEB_PREVIEW) {
    return webPreviewApi as EtymaDesktopApi;
  }
  const api = window.etyma;
  if (!api) {
    throw new Error("Etyma desktop UI requires Electron preload APIs.");
  }
  return api;
}

export async function invoke<K extends DesktopCommand>(
  command: K,
  ...args: DesktopCommandParameters<K>
): Promise<DesktopCommandResult<K>> {
  return getDesktopApi().invoke(command, ...args);
}
