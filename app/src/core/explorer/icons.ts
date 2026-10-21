// Which icon a tree entry gets.

import AudioLines from "@lucide/svelte/icons/audio-lines";
import File from "@lucide/svelte/icons/file";
import FileBraces from "@lucide/svelte/icons/file-braces";
import FileCode from "@lucide/svelte/icons/file-code";
import FileCog from "@lucide/svelte/icons/file-cog";
import FileSpreadsheet from "@lucide/svelte/icons/file-spreadsheet";
import FileText from "@lucide/svelte/icons/file-text";
import Folder from "@lucide/svelte/icons/folder";
import FolderArchive from "@lucide/svelte/icons/folder-archive";
import FolderDot from "@lucide/svelte/icons/folder-dot";
import FolderGit from "@lucide/svelte/icons/folder-git-2";
import FolderOpen from "@lucide/svelte/icons/folder-open";
import FolderX from "@lucide/svelte/icons/folder-x";
import Image from "@lucide/svelte/icons/image";
import Video from "@lucide/svelte/icons/video";

import type { Category, ProjectMarker, TreeNode } from "../../api";

type Icon = typeof Folder;

export function iconOfCategory(category: Category): Icon {
  switch (category) {
    case "markdown":
      return FileText;
    case "text":
    case "other":
      return File;
    case "config":
      return FileCog;
    case "data":
      return FileBraces;
    case "image":
      return Image;
    case "audio":
      return AudioLines;
    case "video":
      return Video;
    case "office":
      return FileSpreadsheet;
    case "code":
      return FileCode;
    default:
      return category satisfies never;
  }
}

export function iconOf(node: TreeNode, open: boolean): Icon {
  if (node.kind.type === "file") return iconOfCategory(node.kind.category);
  const unlisted = node.kind.unlisted;
  if (unlisted === null) return open ? FolderOpen : Folder;
  switch (unlisted.type) {
    case "large":
      return FolderArchive;
    case "ignored":
      return FolderX;
    default:
      return unlisted satisfies never;
  }
}

/** The icon of a worktree section. */
export const WorktreeIcon: Icon = FolderGit;

/** The icon of an included project (shape says it, not only its hue): a repository | a folder with PerMark settings. */
export function projectIcon(marker: ProjectMarker | null): Icon {
  return marker === "permark" ? FolderDot : FolderGit;
}
