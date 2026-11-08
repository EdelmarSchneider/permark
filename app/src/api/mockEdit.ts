// Editing for the mock backend: the editor reads and writes the files of the workspace shown (the project's files are
// the mock project's own: a move, a delete or a link rewrite is what the editor sees next). Markdown block maps come
// from the real backend (`generated/mock-edit-sources.json`, written by `cargo test`); a save checks and applies line
// edits like `edit/mod.rs` + `edit/lines.rs`.

import { applyEdits } from "../core/editing/lines";
import { MOCK_EDIT_SOURCES, mockView } from "./mockDocs";
import { seedFiles, storedPath } from "./mockFs";
import type { Encoding } from "./generated/Encoding";
import type { ErrorKind } from "./generated/ErrorKind";
import type { LineEnding } from "./generated/LineEnding";
import type { Api, LineEdit, Measure, Result, SaveOutcome, Segment } from "./types";

const ok = <T>(value: T): Promise<Result<T>> => Promise.resolve({ ok: true, value });

const BOM = "﻿";

/** FNV-1a (two lanes) as hex 16: stands in for the backend's xxh3. */
export function mockHash(text: string): string {
  let a = 0x811c9dc5;
  let b = 0x01000193;
  for (let i = 0; i < text.length; i++) {
    const c = text.charCodeAt(i);
    a = Math.imul(a ^ c, 0x01000193) >>> 0;
    b = Math.imul(b ^ (c + 31), 0x01000193) >>> 0;
  }
  return a.toString(16).padStart(8, "0") + b.toString(16).padStart(8, "0");
}

/** Blank-line separated runs (the mock has no parser). */
export function mockSegments(text: string): Segment[] {
  const lines = text.split("\n");
  const out: Segment[] = [];
  let start = -1;
  const close = (end: number): void => {
    out.push({ lineStart: start + 1, lineEnd: end, kind: "paragraph", lock: null });
    start = -1;
  };
  lines.forEach((line, i) => {
    const filled = line.trim() !== "";
    if (filled && start < 0) start = i;
    if (!filled && start >= 0) close(i);
  });
  if (start >= 0) close(lines.length);
  return out;
}

/** A file's text as the backend decodes it: BOM gone, its encoding, its line ending (`fsx::line_ending`), the break
 *  most lines use (`lines::majority`) and the edit text (every break `\n`). */
export function mockDecoded(raw: string): {
  body: string;
  text: string;
  encoding: Encoding;
  lineEnding: LineEnding;
  majority: string;
} {
  const bom = raw.startsWith(BOM);
  const body = bom ? raw.slice(BOM.length) : raw;
  const crlf = body.split("\r\n").length - 1;
  const cr = (body.match(/\r(?!\n)/g) ?? []).length;
  const lf = (body.match(/(?<!\r)\n/g) ?? []).length;
  const lineEnding: LineEnding =
    lf > 0 && crlf > 0 ? "mixed" : crlf > 0 ? "crLf" : lf > 0 ? "lf" : "none";
  const majority = crlf > lf && crlf >= cr ? "\r\n" : cr > lf && cr > crlf ? "\r" : "\n";
  return {
    body,
    text: body.replace(/\r\n?/g, "\n"),
    encoding: bom ? "utf8Bom" : "utf8",
    lineEnding,
    majority,
  };
}

/** Bytes of an edit text in a file's ways: its break on every line, its BOM. (The backend keeps each kept line's own
 *  break; the mock's files use one kind each.) */
function encoded(text: string, ending: string, encoding: Encoding): string {
  const body = ending === "\n" ? text : text.replace(/\n/g, ending);
  return encoding === "utf8Bom" ? BOM + body : body;
}

/** Why edits cannot apply to a text of `count` lines, as `lines::apply` refuses them; `null`: they can. */
function editProblem(
  count: number,
  edits: readonly LineEdit[],
): "outOfRange" | "order" | "lineBreak" | null {
  let cursor = 0;
  for (const edit of edits) {
    const end = edit.at + edit.remove;
    if (end > count) return "outOfRange";
    if (edit.at < cursor) return "order";
    if (edit.insert.some((line) => line.includes("\n") || line.includes("\r"))) return "lineBreak";
    cursor = end;
  }
  return null;
}

/** A read | save the mock fails on request (a file that is binary, too large, read-only, ...). */
export type MockEditFailure = {
  command: "edit_source" | "edit_save";
  path: string;
  kind: ErrorKind;
  detail: string | null;
};

export type MockEditProbe = {
  /** Saves made: path + edits. */
  saves: { path: string; edits: LineEdit[] }[];
  /** Last `editSetUnsaved`. */
  unsaved: string | null;
  /** `windowClose` was called. */
  closed: boolean;
  /** Files as the editor sees them: those of the workspace shown (the project's are the mock project's files). */
  readonly files: Map<string, string>;
  /** Reads | saves that fail with a kind, until taken out of the list. */
  failures: MockEditFailure[];
};

type EditApi = Pick<
  Api,
  | "editSource"
  | "editSegments"
  | "editSave"
  | "editSaveAs"
  | "editWriteAsset"
  | "editSetUnsaved"
  | "editHeadings"
  | "editLinkTargets"
  | "analyze"
  | "windowClose"
>;

/** What the editing mock asks of the rest of the mock backend. */
export type MockEditHost = {
  /** The files of the workspace shown. */
  files: () => Map<string, string>;
  /** Paths link completion offers. */
  linkTargets: () => string[];
  /** A save wrote `path` as `hash` (the window that saved hears no change back). */
  saved: (path: string, hash: string) => void;
  /** A new file was written. */
  created: (path: string) => void;
  /** The unsaved flag changed (the tab's chip shows it). */
  unsaved: () => void;
};

/** A host of its own: the mock project's seed files, nothing else told. */
function standalone(): MockEditHost {
  const files = new Map(Object.entries(seedFiles()));
  return {
    files: () => files,
    linkTargets: () => [...files.keys()].sort(),
    saved: () => {},
    created: () => {},
    unsaved: () => {},
  };
}

export function createMockEditing(host: MockEditHost = standalone()): {
  api: EditApi;
  probe: MockEditProbe;
} {
  const probe: MockEditProbe = {
    saves: [],
    unsaved: null,
    closed: false,
    get files() {
      return host.files();
    },
    failures: [],
  };
  const refuse = <T>(
    command: "edit_source" | "edit_save" | "edit_save_as",
    kind: ErrorKind,
    subject: string,
    detail: string | null = null,
  ): Promise<Result<T>> =>
    Promise.resolve({
      ok: false,
      error: { kind: "backend", command, error: { kind, subject, detail } },
    });
  const failure = <T>(
    command: MockEditFailure["command"],
    path: string,
  ): Promise<Result<T>> | null => {
    const found = probe.failures.find((f) => f.command === command && f.path === path);
    return found ? refuse(command, found.kind, path, found.detail) : null;
  };
  /** Something is at `path` (a file or a folder holding files), in any case. */
  const taken = (files: Map<string, string>, path: string): boolean => {
    const key = path.toLowerCase();
    return [...files.keys()].some((p) => {
      const lower = p.toLowerCase();
      return lower === key || lower.startsWith(`${key}/`);
    });
  };

  const api: EditApi = {
    editSource: (path) => {
      const files = host.files();
      const stored = storedPath(files, path);
      if (stored === null) return refuse("edit_source", "notFound", path);
      const failed = failure<never>("edit_source", path);
      if (failed) return failed;
      const raw = files.get(stored) ?? "";
      const file = mockDecoded(raw);
      const markdown = /\.(md|markdown|mdown|mdx)$/i.test(path);
      const known = MOCK_EDIT_SOURCES.get(stored);
      const segments = known?.text === file.text ? known.segments : mockSegments(file.text);
      return ok({
        path,
        text: file.text,
        hash: mockHash(raw),
        lineEnding: file.lineEnding,
        encoding: file.encoding,
        segments: markdown ? segments : null,
      });
    },
    editSegments: (text) => ok(mockSegments(text)),
    editSave: (path, seenHash, edits) => {
      const files = host.files();
      const stored = storedPath(files, path);
      // gone (deleted, moved, renamed): the person decides what happens to the text
      if (stored === null) return ok<SaveOutcome>({ outcome: "conflict", diskHash: null });
      const failed = failure<SaveOutcome>("edit_save", path);
      if (failed) return failed;
      const raw = files.get(stored) ?? "";
      const disk = mockHash(raw);
      if (disk !== seenHash) return ok<SaveOutcome>({ outcome: "conflict", diskHash: disk });
      const file = mockDecoded(raw);
      const problem = editProblem(file.text.split("\n").length, edits);
      if (problem !== null) return refuse("edit_save", "invalidData", path, problem);
      const next = encoded(applyEdits(file.text, edits), file.majority, file.encoding);
      if (next === raw)
        return ok<SaveOutcome>({ outcome: "saved", hash: disk, mtimeMs: Date.now() });
      files.set(stored, next);
      probe.saves.push({ path, edits });
      const hash = mockHash(next);
      host.saved(path, hash);
      return ok<SaveOutcome>({ outcome: "saved", hash, mtimeMs: Date.now() });
    },
    editSaveAs: (path, text, exact) => {
      const files = host.files();
      const original = storedPath(files, path);
      const file = original === null ? null : mockDecoded(files.get(original) ?? "");
      const bytes = encoded(text, file?.majority ?? "\n", file?.encoding ?? "utf8");
      const slash = path.lastIndexOf("/");
      const dir = slash >= 0 ? path.slice(0, slash + 1) : "";
      const name = path.slice(slash + 1);
      const dot = name.lastIndexOf(".");
      const stem = dot > 0 ? name.slice(0, dot) : name;
      const ext = dot > 0 ? name.slice(dot) : "";
      const candidates = exact
        ? [path]
        : Array.from(
            { length: 999 },
            (_, i) => `${dir}${stem}-mine${i === 0 ? "" : `-${i + 1}`}${ext}`,
          );
      for (const target of candidates) {
        if (taken(files, target)) {
          if (exact) return refuse("edit_save_as", "exists", target);
          continue;
        }
        files.set(target, bytes);
        host.created(target);
        return ok(target);
      }
      return refuse("edit_save_as", "exists", path);
    },
    editWriteAsset: (doc, ext, _bytes, folder) => {
      const slash = doc.lastIndexOf("/");
      const dir = slash >= 0 ? doc.slice(0, slash) : "";
      const file = slash >= 0 ? doc.slice(slash + 1) : doc;
      const dot = file.lastIndexOf(".");
      const stem = dot > 0 ? file.slice(0, dot) : file;
      const name = `${stem}-1.${ext === "jpeg" ? "jpg" : ext}`;
      const trimmed = folder.trim();
      const parts = trimmed === "" ? [dir, name] : [dir, trimmed, name];
      return ok(parts.filter((part) => part !== "").join("/"));
    },
    editSetUnsaved: (path) => {
      probe.unsaved = path;
      host.unsaved();
      return ok(null);
    },
    editHeadings: (path) => {
      const view = mockView(path, [], true);
      return ok(view?.body.kind === "markdown" ? view.body.outline : []);
    },
    editLinkTargets: () => ok(host.linkTargets()),
    analyze: (_path, text) => {
      const words = (text ?? "").split(/\s+/).filter((w) => w !== "").length;
      const measures: Measure[] = [{ id: "word-count.words", value: words, unit: "words" }];
      return ok(measures);
    },
    windowClose: () => {
      probe.closed = true;
      return ok(null);
    },
  };
  return { api, probe };
}
