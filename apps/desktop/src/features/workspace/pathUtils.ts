export function fileNameFromPath(value: string): string {
  return value.split(/[\\/]/).filter(Boolean).pop() ?? value;
}
