// What the reading view hands the enhancement pass so that code references become links: which project file a written path
// means, and the words of the tooltips. The file list is the one the Explorer holds, so nothing is read from disk.

import { buildFileIndex, resolveCodeRef, type FileIndex } from "../codeRefs";
import { t } from "../i18n";
import { isMarkdown } from "../paths";
import type { CodeRefsContext } from "./codeRefsDom";

let cached: { files: readonly string[]; index: FileIndex } | null = null;

/** The index of a list of files, built once for as long as the same list is in use. */
function indexOf(files: readonly string[]): FileIndex {
  if (cached?.files !== files) cached = { files, index: buildFileIndex(files) };
  return cached.index;
}

/** The tooltip of a reference to one file. A Markdown file opens here; any other file goes to the editor. */
export function refTitle(path: string, line: number | null): string {
  if (isMarkdown(path)) {
    return line === null
      ? t("codeRef.titleDoc", { path })
      : t("codeRef.titleDocAt", { path, line });
  }
  return line === null ? t("codeRef.title", { path }) : t("codeRef.titleAt", { path, line });
}

/** The references of the document `docPath`, looked up among `files` (the project's files, as the Explorer lists them). */
export function codeRefsContextFor(files: readonly string[], docPath: string): CodeRefsContext {
  const index = indexOf(files);
  return {
    resolve: (written) => resolveCodeRef(index, docPath, written),
    title: refTitle,
    titleMany: (count) => t("codeRef.titleMany", { count }),
  };
}
