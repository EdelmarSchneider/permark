// Editors the settings offer (command templates; the person can write their own). `{path}`, `{line}`, `{col}` are
// filled by the backend; no shell is used.

import type { MessageKey } from "../i18n";

export const EDITOR_PRESETS: readonly { id: string; label: MessageKey; command: string }[] = [
  { id: "vscode", label: "settings.editor.vscode", command: "code -g {path}:{line}:{col}" },
  { id: "cursor", label: "settings.editor.cursor", command: "cursor -g {path}:{line}:{col}" },
  { id: "zed", label: "settings.editor.zed", command: "zed {path}:{line}:{col}" },
  { id: "sublime", label: "settings.editor.sublime", command: "subl {path}:{line}:{col}" },
  { id: "jetbrains", label: "settings.editor.jetbrains", command: "idea --line {line} {path}" },
];

/** Which choice a command is: `none` (empty), a preset's id, or `custom`. */
export function presetOf(command: string): string {
  const text = command.trim();
  if (text === "") return "none";
  return EDITOR_PRESETS.find((p) => p.command === text)?.id ?? "custom";
}

/** A command as it would run for a sample file (what the settings show under the field). */
export function editorPreview(command: string): string {
  return command
    .trim()
    .replaceAll("{path}", "/docs/plan.md")
    .replaceAll("{line}", "12")
    .replaceAll("{col}", "1");
}
