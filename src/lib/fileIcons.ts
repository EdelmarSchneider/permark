// Which icon stands for a file in the Explorer and in the lists of Quick open and Search everywhere.

import { assetKindOf } from "./fileKinds";
import { isMarkdown } from "./paths";

export type FileIcon = "file-text" | "file" | "image" | "music" | "video";

/** A document, a picture, a sound and a video each have their own; any other file is a plain sheet. */
export function fileIconOf(path: string): FileIcon {
  if (isMarkdown(path)) return "file-text";
  switch (assetKindOf(path)) {
    case "image":
      return "image";
    case "audio":
      return "music";
    case "video":
      return "video";
    default:
      return "file";
  }
}
