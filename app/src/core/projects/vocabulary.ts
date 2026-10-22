// Official tags and categories as the UI treats them (pure; `projects.md` §7): the words of a status mark, whether the
// opened folder's official list can take a new name now (and why not), where it is saved, and the words of a refusal.
// The rules themselves (which list a name is in) live in the backend; the UI only shows what it is told.

import type { ApiError, ProjectsView, TagStatus, VocabularyEntry, VocabularyKind } from "../../api";
import { apiErrorText } from "../errors";
import { t, type MessageKey } from "../i18n";

/** The names of a kind in the official list (empty while there is none). */
export function officialOf(view: ProjectsView | null, kind: VocabularyKind): VocabularyEntry[] {
  if (!view) return [];
  return kind === "tag" ? view.vocabulary.tags : view.vocabulary.categories;
}

/** The official entry of a name (`null`: not official). Names arrive normalized from the backend. */
export function officialEntry(
  view: ProjectsView | null,
  kind: VocabularyKind,
  name: string,
): VocabularyEntry | null {
  return officialOf(view, kind).find((e) => e.name === name) ?? null;
}

/** The short name of the project that lists a local name (`null`: unknown | no such row). */
export function ownerShort(view: ProjectsView | null, uid: string | null): string | null {
  if (!view || uid === null) return null;
  return view.rows.find((r) => r.uid === uid)?.short ?? null;
}

/** The words of a status mark: tooltip + what a screen reader hears after the name. */
export function statusWords(status: TagStatus, kind: VocabularyKind, short: string | null): string {
  switch (status) {
    case "official":
      return t("tags.status.official");
    case "local":
      return short === null ? t("tags.status.localAny") : t("tags.status.local", { short });
    case "unlisted":
      return t(kind === "tag" ? "tags.status.unlisted" : "projects.category.unlisted");
    default:
      return status satisfies never;
  }
}

/**
 * Why the official list can't take a name now (`null`: it can). The list lives in the opened folder's `.permark/`,
 * which exists once a project is included (record present) or a list was written before; a list that can't be read |
 * that a newer PerMark wrote is never written over (the backend refuses too).
 */
export function promoteBlocked(view: ProjectsView | null): string | null {
  if (!view || !view.present) return t("projects.vocabulary.unavailable");
  const file = view.vocabulary.file;
  if (file.status === "unreadable") return t("projects.vocabulary.unreadable", { line: file.line });
  if (file.status === "newer") return t("projects.vocabulary.newer");
  if (view.record.status === "unreadable" || view.record.status === "newer") {
    return t("projects.vocabulary.unavailable");
  }
  if (view.record.status === "absent" && file.status === "absent") {
    return t("projects.vocabulary.unavailable");
  }
  return null;
}

/** Where a promote writes, said before it does: the file is made | grows. */
export function vocabularyPlace(view: ProjectsView | null): MessageKey {
  return view?.vocabulary.file.status === "ok"
    ? "projects.vocabulary.saves"
    : "projects.vocabulary.creates";
}

/** The words of a refused promote: the list changed under the save | is not writable now | anything else by kind
 *  (a read-only `.permark/` arrives as an I/O error with the system's reason). */
export function vocabularyErrorText(error: ApiError): string {
  if (error.kind === "backend" && error.error.subject === "vocabulary") {
    if (error.error.kind === "conflict") return t("projects.vocabulary.conflict");
    if (error.error.kind === "unavailable") return t("projects.vocabulary.unavailable");
  }
  return apiErrorText(error);
}

/** A typed name as the backend will keep it (the tag rules: trimmed, lowercased, spaces -> "-", 40 characters). */
export function vocabularyName(raw: string): string | null {
  const joined = raw
    .split(/\s+/)
    .filter((w) => w !== "")
    .join("-")
    .toLowerCase();
  const name = [...joined].slice(0, 40).join("");
  return name === "" ? null : name;
}
