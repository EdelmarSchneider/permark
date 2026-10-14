// New file (docs/design/v0.2.md section 3): the name typed in the tree, where the file goes, and what it starts with.
// The backend checks the same rules; these let the input say what is wrong before anything is sent.

import type { MessageKey } from "../i18n";
import { basename, dirname, splitWorktree } from "../paths";

export const DEFAULT_NEW_NAME = "untitled.md";

const MARKDOWN = /\.(md|markdown|mdown|mkd|mdx)$/i;
const FORBIDDEN = /[\\/:*?"<>|\u0000-\u001f]/;
const RESERVED = new Set([
  "con",
  "prn",
  "aux",
  "nul",
  ...Array.from({ length: 9 }, (_, i) => `com${i + 1}`),
  ...Array.from({ length: 9 }, (_, i) => `lpt${i + 1}`),
]);

/** `.md` is added when the name has no extension ("notes" becomes "notes.md"). */
export function withExtension(typed: string): string {
  const name = typed.trim();
  if (name === "") return name;
  const dot = name.lastIndexOf(".");
  const hasExtension = dot > 0 && dot < name.length - 1;
  return hasExtension ? name : `${name.replace(/\.+$/, "")}.md`;
}

export type NameProblem =
  "empty" | "invalidChars" | "invalidEnd" | "reserved" | "tooLong" | "notMarkdown" | "exists";

export const NAME_PROBLEM_TEXT: Record<NameProblem, MessageKey> = {
  empty: "newFile.empty",
  invalidChars: "newFile.invalidChars",
  invalidEnd: "newFile.invalidEnd",
  reserved: "newFile.reserved",
  tooLong: "newFile.tooLong",
  notMarkdown: "newFile.notMarkdown",
  exists: "newFile.exists",
};

/**
 * What is wrong with a file name, or null. `name` is the final name (see [`withExtension`]); `siblings` are the
 * names already in the folder, compared without regard to case.
 */
export function nameProblem(name: string, siblings: readonly string[]): NameProblem | null {
  if (name.trim() === "") return "empty";
  if (FORBIDDEN.test(name)) return "invalidChars";
  if (/[. ]$/.test(name) || name.startsWith(" ")) return "invalidEnd";
  if (!MARKDOWN.test(name) || name.startsWith(".")) return "notMarkdown";
  // spaces before the extension do not help: Windows reads `con .md` as the device too
  if (RESERVED.has(name.split(".")[0].trimEnd().toLowerCase())) return "reserved";
  if (new TextEncoder().encode(name).length > 255) return "tooLong";
  const lower = name.toLowerCase();
  if (siblings.some((s) => s.toLowerCase() === lower)) return "exists";
  return null;
}

/** Where the typed stem starts and ends in `name`, so the input can select it (`untitled` in `untitled.md`). */
export function stemRange(name: string): [number, number] {
  const dot = name.lastIndexOf(".");
  return [0, dot > 0 ? dot : name.length];
}

/** The first of `untitled.md`, `untitled-2.md`... that is not taken in the folder. */
export function freshName(siblings: readonly string[]): string {
  const taken = new Set(siblings.map((s) => s.toLowerCase()));
  if (!taken.has(DEFAULT_NEW_NAME)) return DEFAULT_NEW_NAME;
  for (let n = 2; ; n++) {
    const name = `untitled-${n}.md`;
    if (!taken.has(name)) return name;
  }
}

/** The title a new file starts with: its name without the extension, dashes and underscores as spaces. */
export function titleOf(name: string): string {
  const stem = name.replace(MARKDOWN, "");
  return stem.replace(/[-_]+/g, " ").trim() || stem;
}

/** The blank template: a title and a final newline. */
export function blankTemplate(name: string): string {
  return `# ${titleOf(name)}\n`;
}

/** The vault path of a new file. */
export function newFilePath(folder: string, name: string): string {
  return folder === "" ? name : `${folder}/${name}`;
}

/**
 * The folder a new file goes into: the folder of the selected tree row (a folder row is the folder itself), else the
 * folder of the open document, else the project root.
 */
export function targetFolder(
  selected: { path: string; isDir: boolean } | null,
  openDoc: string | null,
): string {
  if (selected) {
    // a worktree row is the worktree's own root
    const { worktree, rel } = splitWorktree(selected.path);
    if (selected.isDir || (worktree !== null && rel === "")) return selected.path;
    return dirname(selected.path);
  }
  return openDoc ? dirname(openDoc) : "";
}

/** The names (not paths) of the files directly inside `folder`. */
export function namesIn(folder: string, files: readonly string[]): string[] {
  return files.filter((p) => dirname(p) === folder).map(basename);
}
