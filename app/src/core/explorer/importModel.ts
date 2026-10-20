// Drop from outside, the parts that need no state: where a drop goes (route), what the badge and the question say,
// how an outcome is worded (`explorer.md` Drop from outside).

import type { DropOpened, ImportOutcome, ImportPlan, ImportRefusal } from "../../api";
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

// ---- route ------------------------------------------------------------------------------------------------------------

/** What is under the pointer: the Explorer list (the folder a drop there lands in), an editor being edited (whether
 *  it takes pictures), or anything else. */
export type DropSpot =
  { area: "explorer"; folder: string } | { area: "editor"; pictures: boolean } | { area: "other" };

/** What the view shows, and which files the Notes open as loose files (`null`: the Notes are not shown). */
export type DropContext = {
  workspace: "project" | "singleFile" | "notes" | "welcome";
  notesTake: ((path: string) => boolean) | null;
};

export type DropRoute =
  /** Every path a picture, over an editor that takes them: inserted at the pointer. */
  | { kind: "pictures" }
  /** Over the Explorer list of a project: into `folder` (the question asks Copy | Move). */
  | { kind: "import"; folder: string }
  /** On the Notes tab: `loose` become loose files, `open` open as tabs. */
  | { kind: "notes"; loose: string[]; open: string[] }
  /** Anywhere else: each path opens as a tab. */
  | { kind: "open" };

/** Where a drop goes; first match wins. */
export function routeDrop(
  spot: DropSpot,
  paths: readonly string[],
  context: DropContext,
): DropRoute {
  if (spot.area === "editor" && spot.pictures && paths.length > 0 && paths.every(isPicturePath)) {
    return { kind: "pictures" };
  }
  if (context.workspace === "project" && spot.area === "explorer") {
    return { kind: "import", folder: spot.folder };
  }
  if (context.workspace === "notes") {
    const take = context.notesTake ?? (() => false);
    return {
      kind: "notes",
      loose: paths.filter((p) => take(p)),
      open: paths.filter((p) => !take(p)),
    };
  }
  return { kind: "open" };
}

/** More paths than this open only after a question. */
export const OPEN_ASK_ABOVE = 8;

// ---- while dragging ---------------------------------------------------------------------------------------------------

export type BadgeIcon = "import" | "open" | "pictures" | "notes";
export type Badge = { icon: BadgeIcon; text: string };

/** A long name shortened in the middle (both ends stay readable). */
export function middle(name: string, max = 32): string {
  const chars = Array.from(name);
  if (chars.length <= max) return name;
  const head = Math.ceil((max - 1) / 2);
  const tail = max - 1 - head;
  return `${chars.slice(0, head).join("")}…${chars.slice(chars.length - tail).join("")}`;
}

/** What the badge near the pointer says for a route (`folder`: the import target as the person reads it; `busy`: an
 *  import is running in this view). */
export function badgeOf(route: DropRoute, count: number, folder: string, busy: boolean): Badge {
  switch (route.kind) {
    case "import":
      return busy
        ? { icon: "import", text: t("import.busy") }
        : { icon: "import", text: tn("drop.badge.add", count, { folder: middle(folder) }) };
    case "open":
      return { icon: "open", text: tn("drop.badge.open", count) };
    case "pictures":
      return { icon: "pictures", text: tn("drop.badge.pictures", count) };
    case "notes":
      if (route.loose.length === 0) return { icon: "open", text: tn("drop.badge.open", count) };
      return route.open.length === 0
        ? { icon: "notes", text: tn("drop.badge.notes", count) }
        : { icon: "notes", text: t("drop.badge.mixed", { n: count }) };
    default:
      return route satisfies never;
  }
}

// ---- the question -----------------------------------------------------------------------------------------------------

/** A folder as the question names it: its own name, or the project's name for the project folder. */
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

/** "2 files and 1 folder": what can come in (refused entries not counted). */
export function countsText(plan: ImportPlan): string {
  const folders = plan.items.filter((i) => i.kind === "folder").length;
  const files = plan.items.length - folders;
  if (folders === 0) return tn("import.kinds.files", files);
  if (files === 0) return tn("import.kinds.folders", folders);
  return t("import.kinds.both", {
    files: tn("import.kinds.files", files),
    folders: tn("import.kinds.folders", folders),
  });
}

/** The first `shown` of a list, then "and n more". */
export function firstOf(names: readonly string[], shown: number): string {
  const listed = names.slice(0, shown).join(", ");
  return names.length > shown ? `${listed} ${tn("import.more", names.length - shown)}` : listed;
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

// ---- after ------------------------------------------------------------------------------------------------------------

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

/** The one notice of a drop whose paths partly failed to open (`null`: none failed). */
export function openFailedText(failed: DropOpened["failed"]): string | null {
  const first = failed[0];
  if (first === undefined) return null;
  return tn("drop.openFailed", failed.length, {
    name: baseName(first.path),
    reason: ipcErrorText(first.error),
  });
}
