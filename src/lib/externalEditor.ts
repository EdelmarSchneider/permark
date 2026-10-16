// The "External editor" setting: the command that opens a file at a line. The presets, how a saved command maps back to a
// preset, the preview shown under the field, and which files skip the editor. The command itself is split and run by the
// backend (`src-tauri/src/external_editor.rs`), which also decides whether it is valid.

import { categoryOf } from "./fileKinds";
import { extname } from "./paths";

/** The editors the Settings list offers. `system` has no command: the file opens with the system's default application. */
export const EDITOR_PRESETS = [
  { id: "system", command: "" },
  { id: "vscode", command: "code -g {path}:{line}:{col}" },
  { id: "cursor", command: "cursor -g {path}:{line}:{col}" },
  { id: "zed", command: "zed {path}:{line}:{col}" },
  { id: "sublime", command: "subl {path}:{line}:{col}" },
  { id: "jetbrains", command: "idea --line {line} {path}" },
] as const;

export type EditorPresetId = (typeof EDITOR_PRESETS)[number]["id"] | "custom";

/** The preset ids in the order of the list, the free text last. */
export const EDITOR_CHOICES: readonly EditorPresetId[] = [
  ...EDITOR_PRESETS.map((preset) => preset.id),
  "custom",
];

/** The preset a command is, or `custom`. Only spacing around the command is ignored. */
export function presetOf(command: string): EditorPresetId {
  const wanted = command.trim();
  return EDITOR_PRESETS.find((preset) => preset.command === wanted)?.id ?? "custom";
}

/** The command a preset fills in; null for `custom`, which keeps whatever is typed. */
export function commandOf(preset: EditorPresetId): string | null {
  return EDITOR_PRESETS.find((p) => p.id === preset)?.command ?? null;
}

/**
 * The command with a sample file filled in, as a hint of what will run. A path with spaces is shown in quotes: the
 * backend passes it as one argument whatever it holds.
 */
export function previewCommand(command: string, path: string, line: number, col: number): string {
  const shown = /\s/.test(path) ? `"${path}"` : path;
  return command
    .trim()
    .replaceAll("{path}", shown)
    .replaceAll("{line}", String(line))
    .replaceAll("{col}", String(col));
}

/** A file of the project as the system writes its path, for the preview (the folder's own separators are used). */
export function sampleFile(root: string | null | undefined, relative: string): string {
  if (!root) return relative;
  const sep = root.includes("\\") ? "\\" : "/";
  return root.replace(/[\\/]+$/, "") + sep + relative.split("/").join(sep);
}

/**
 * Files the system's default application opens better than a code editor: pictures, sound, video and PDF files. "Open in
 * external editor" hands those to it whatever command is set.
 */
export function usesSystemApp(path: string): boolean {
  const category = categoryOf(path);
  return category === "images" || category === "media" || extname(path) === "pdf";
}
