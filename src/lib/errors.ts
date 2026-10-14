// Turns whatever a rejected backend call carries into a short message for the UI.
// Tauri rejects with the serialized `AppError`, which may be a string or an object with a message.

import { MESSAGES, t, type MessageKey, type Params } from "./i18n";

/** The stable `kind` the backend gave an error (`exists`, `denied`, `notFound`...), or null when it has none. */
export function errorKind(error: unknown): string | null {
  if (error && typeof error === "object") {
    const kind = (error as Record<string, unknown>).kind;
    if (typeof kind === "string") return kind;
  }
  return null;
}

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  if (error && typeof error === "object") {
    const record = error as Record<string, unknown>;
    for (const key of ["message", "msg", "error", "detail"]) {
      const value = record[key];
      if (typeof value === "string" && value !== "") return value;
    }
    const entries = Object.entries(record);
    // serde externally tagged enums look like { notFound: "path" } or { io: { message } }
    if (entries.length === 1) {
      const [tag, value] = entries[0];
      if (typeof value === "string") return `${tag}: ${value}`;
      if (value && typeof value === "object") return errorText(value);
    }
    try {
      return JSON.stringify(error);
    } catch {
      return String(error);
    }
  }
  return String(error);
}

// ---- wording errors in the language of the interface ---------------------------------------------------------------
// The backend reports `{ kind, message, subject?, detail?, reason? }`. `message` is English, for logs; the UI words
// the kinds it knows with its own strings (`error.<kind>`) and fills them from `subject` (a path or a name),
// `detail` (diagnostic text from the system, shown as it is) and `reason` (a code that narrows `invalidName`).

/** The keys that word each reason an `invalidName` error gives. */
const NAME_REASONS = {
  empty: "fileOps.nameEmpty",
  forbiddenChars: "fileOps.nameInvalidChars",
  edgeSpaceOrDot: "fileOps.nameInvalidEnd",
  noStem: "error.nameNoStem",
  reserved: "fileOps.nameReserved",
  tooLong: "fileOps.nameTooLong",
  notImage: "images.onlyImages",
  emptyImage: "error.imageEmpty",
} as const satisfies Record<string, MessageKey>;

/** The kinds the UI words itself, and the string for each (the same name under `error.`, except where one exists already). */
const KIND_KEYS = {
  noVault: "error.noVault",
  superseded: "error.superseded",
  internal: "error.internal",
  pathEscape: "error.pathEscape",
  invalidPath: "error.invalidPath",
  notFound: "error.notFound",
  io: "error.io",
  denied: "error.denied",
  watch: "error.watch",
  git: "error.git",
  gitUnavailable: "error.gitUnavailable",
  session: "error.session",
  metaUnreadable: "error.metaUnreadable",
  metaWorktree: "tags.worktreeReadOnly",
  binary: "error.binary",
  tooLarge: "error.tooLarge",
  notMarkdown: "error.notMarkdown",
  encoding: "error.encoding",
  exists: "error.exists",
  unsupported: "error.unsupported",
  invalidData: "error.invalidData",
  intoItself: "error.intoItself",
  otherRoot: "error.otherRoot",
  protected: "error.protected",
  holdsWorktree: "error.holdsWorktree",
  overlap: "error.overlap",
  simpleMode: "fileOps.simpleMode",
  nothing: "error.nothing",
  unknownPlan: "error.unknownPlan",
  unknownOperation: "error.unknownOperation",
  partlyMoved: "error.partlyMoved",
  trash: "error.trash",
  shell: "error.shell",
} as const satisfies Record<string, MessageKey>;

function field(error: Record<string, unknown>, name: string): string | undefined {
  const value = error[name];
  return typeof value === "string" ? value : undefined;
}

/**
 * Words an error for the person, in the language of the interface. Errors of a kind the UI has words for are
 * written with them; anything else (a library's error, a kind a newer backend added, a sentence that lacks a
 * blank it needs) keeps the backend's own text, which `errorText` finds.
 */
export function errorMessage(error: unknown): string {
  if (error && typeof error === "object") {
    const record = error as Record<string, unknown>;
    const kind = errorKind(error);
    const subject = field(record, "subject");
    const detail = field(record, "detail");
    const reason = field(record, "reason");
    let key: MessageKey | undefined;
    if (kind === "invalidName") {
      key = reason !== undefined ? (NAME_REASONS as Record<string, MessageKey>)[reason] : undefined;
    } else if (kind !== null) {
      key = (KIND_KEYS as Record<string, MessageKey>)[kind];
    }
    if (key !== undefined) {
      const template = t(key);
      const blanks = [...template.matchAll(/\{(\w+)\}/g)].map((m) => m[1]);
      const params: Record<string, string | undefined> = {
        subject,
        detail,
        // `meta.toml` says where its first mistake is (the subject holds the line)
        where:
          kind === "metaUnreadable" && subject
            ? t("tags.metaUnreadableLine", { line: subject })
            : "",
      };
      if (blanks.every((name) => params[name] !== undefined)) return t(key, params as Params);
    }
  }
  return errorText(error);
}

/** The words for the notes the backend attaches to a document (`utf16Le,deepNesting`), joined into one line. */
export function noteText(codes: string): string {
  return codes
    .split(",")
    .map((code) => {
      const key = `note.${code}` as MessageKey;
      return key in MESSAGES.en ? t(key) : code;
    })
    .join("; ");
}
