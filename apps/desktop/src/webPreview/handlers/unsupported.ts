import type {
  DesktopCommand,
  DesktopCommandArgs,
  DesktopCommandResult,
} from "@/appTypes";

/** Commands that are not part of the browser demo surface. */
export function unsupportedWebPreviewCommand(command: DesktopCommand): never {
  throw new Error(
    `Web preview does not support "${command}". Run the Electron desktop app for this action.`,
  );
}

export function makeUnsupportedHandler<K extends DesktopCommand>(
  command: K,
): (args: DesktopCommandArgs<K>) => DesktopCommandResult<K> {
  return () => unsupportedWebPreviewCommand(command);
}
