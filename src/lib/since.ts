// "Since last visit": changed files grouped by folder, newest first.

import { basename, folderLabel } from "./paths";
import type { ChangedSince } from "./types";

export type SinceFile = ChangedSince["files"][number] & { name: string };

export type SinceGroup = {
  /** Folder without worktree prefix; "" is the project root. */
  folder: string;
  files: SinceFile[];
  newestMs: number;
};

export function groupSince(files: ChangedSince["files"]): SinceGroup[] {
  const byFolder = new Map<string, SinceGroup>();
  for (const file of files) {
    const folder = folderLabel(file.path);
    let group = byFolder.get(folder);
    if (!group) {
      group = { folder, files: [], newestMs: 0 };
      byFolder.set(folder, group);
    }
    group.files.push({ ...file, name: basename(file.path) });
    group.newestMs = Math.max(group.newestMs, file.modifiedMs);
  }
  const groups = [...byFolder.values()];
  for (const group of groups) {
    group.files.sort((a, b) => b.modifiedMs - a.modifiedMs || a.name.localeCompare(b.name));
  }
  return groups.sort((a, b) => b.newestMs - a.newestMs || a.folder.localeCompare(b.folder));
}

/** True for the error git reports when the repository belongs to another user. */
export function isDubiousOwnership(gitError: string): boolean {
  return /dubious ownership|safe\.directory/i.test(gitError);
}

/** The command that fixes a "dubious ownership" error for `root`. */
export function safeDirectoryCommand(root: string): string {
  return `git config --global --add safe.directory ${root.replaceAll("\\", "/")}`;
}
