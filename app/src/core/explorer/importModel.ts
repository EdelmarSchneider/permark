// Drop into project, the parts that need no state: where a drop goes, what a plan says, how an outcome is worded.

import type { ImportOutcome, ImportPlan, ImportRefusal } from "../../api";
import { ipcErrorText } from "../errors";
import { t, tn, type MessageKey } from "../i18n";
import { nameOf } from "./list";

const PICTURE = /\.(png|jpe?g|gif|webp|svg)$/i;
const MIME: Record<string, string> = {
  png: "image/png",
  jpg: "image/jpeg",
  jpeg: "image/jpeg",
  gif: "image/gif",
  webp: "image/webp",
  svg: "image/svg+xml",
};

/** Whether a dropped path is a picture the editors take (by its name). */
export function isPicturePath(path: string): boolean {
  return PICTURE.test(path);
}

/** MIME type of a picture path (`""` for anything else). */
export function pictureType(path: string): string {
  const ext = /\.([^./\\]+)$/.exec(path)?.[1]?.toLowerCase() ?? "";
  return MIME[ext] ?? "";
}

/** The file name of an absolute path, either separator. */
export function baseName(path: string): string {
  const parts = path.split(/[\\/]/).filter((p) => p !== "");
  return parts.at(-1) ?? path;
}

export type DropRoute = { kind: "pictures" } | { kind: "import" };

/** Pictures dropped on an editor that is being edited go into the document; everything else is imported. */
export function routeDrop(paths: readonly string[], overEditor: boolean): DropRoute {
  return overEditor && paths.length > 0 && paths.every(isPicturePath)
    ? { kind: "pictures" }
    : { kind: "import" };
}

/** A folder as the dialog names it: its own name, or the project's name for the project folder. */
export function folderLabel(folder: string, project: string): string {
  return folder === "" ? project : nameOf(folder);
}

/** Names that are taken, as the person reads them (folders end in a slash). */
export function conflictNames(plan: ImportPlan): string[] {
  return plan.items
    .filter((i) => i.conflict)
    .map((i) => (i.kind === "folder" ? `${i.name}/` : i.name));
}

/** Items already in the project can be copied here, not moved. */
export function canMove(plan: ImportPlan): boolean {
  return plan.items.every((i) => !i.inProject);
}

const REFUSALS: Record<ImportRefusal, MessageKey> = {
  missing: "import.reason.missing",
  link: "import.reason.link",
  special: "import.reason.special",
  badName: "import.reason.badName",
  protected: "import.reason.protected",
  intoItself: "import.reason.intoItself",
};

export function refusalText(reason: ImportRefusal): string {
  return t(REFUSALS[reason]);
}

/** "Nothing was added" with the first reasons, for a drop where every entry was refused. */
export function nothingText(plan: ImportPlan): string {
  const shown = plan.refused
    .slice(0, 3)
    .map((r) => `${r.name} ${refusalText(r.reason)}`)
    .join("; ");
  return t("import.nothing", { list: shown });
}

/** What happened, in a few plain sentences (`where`: the folder as named for the person). */
export function outcomeText(outcome: ImportOutcome, where: string): string {
  const parts: string[] = [];
  if (outcome.moved > 0) parts.push(tn("import.done.moved", outcome.moved, { folder: where }));
  if (outcome.copied > 0) {
    parts.push(
      tn(outcome.moved > 0 ? "import.done.alsoCopied" : "import.done.copied", outcome.copied, {
        folder: where,
      }),
    );
  }
  if (outcome.skipped > 0) parts.push(tn("import.skipped", outcome.skipped));
  const first = outcome.failed[0];
  if (first) {
    parts.push(
      tn("import.failed", outcome.failed.length, {
        name: first.name,
        reason: ipcErrorText(first.error),
      }),
    );
  }
  if (outcome.links > 0) parts.push(tn("import.links", outcome.links));
  if (outcome.leftBehind > 0) parts.push(tn("import.left", outcome.leftBehind));
  if (outcome.cancelled) {
    parts.push(t(addedAnything(outcome) ? "import.cancelled" : "import.cancelledNone"));
  }
  if (parts.length === 0) parts.push(t("import.noneAdded"));
  return parts.join(" ");
}

/** Whether anything came in (the notice offers Undo then). */
export function addedAnything(outcome: ImportOutcome): boolean {
  return outcome.copied + outcome.moved > 0;
}
