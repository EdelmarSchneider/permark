// Editing for the mock backend: documents as edit texts in memory. Markdown sources come from the real backend
// (`generated/mock-edit-sources.json`, written by `cargo test`); a save applies line edits like the backend does.

import editSources from "./generated/mock-edit-sources.json";
import { mockTextOf, mockView } from "./mockDocs";
import type { Api, EditSource, LineEdit, Measure, Result, SaveOutcome, Segment } from "./types";

const ok = <T>(value: T): Promise<Result<T>> => Promise.resolve({ ok: true, value });

function isSource(value: unknown): value is EditSource {
  return typeof value === "object" && value !== null && "text" in value && "hash" in value;
}

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

function apply(base: string, edits: readonly LineEdit[]): string {
  const lines = base.split("\n");
  const out: string[] = [];
  let next = 0;
  for (const edit of edits) {
    out.push(...lines.slice(next, edit.at), ...edit.insert);
    next = edit.at + edit.remove;
  }
  out.push(...lines.slice(next));
  return out.join("\n");
}

export type MockEditProbe = {
  /** Saves made: path + edits. */
  saves: { path: string; edits: LineEdit[] }[];
  /** Last `editSetUnsaved`. */
  unsaved: string | null;
  /** `windowClose` was called. */
  closed: boolean;
  /** Files as the editor sees them. */
  files: Map<string, string>;
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

export function createMockEditing(linkTargets: () => string[]): {
  api: EditApi;
  probe: MockEditProbe;
} {
  const files = new Map<string, string>();
  const sources = new Map<string, EditSource>();
  for (const [path, source] of Object.entries(editSources)) {
    if (isSource(source)) {
      files.set(path, source.text);
      sources.set(path, source);
    }
  }
  const probe: MockEditProbe = { saves: [], unsaved: null, closed: false, files };
  const textOf = (path: string): string | null => files.get(path) ?? mockTextOf(path);
  const notFound = <T>(command: "edit_source" | "edit_save", path: string): Promise<Result<T>> =>
    Promise.resolve({
      ok: false,
      error: { kind: "backend", command, error: { kind: "notFound", subject: path, detail: null } },
    });

  const api: EditApi = {
    editSource: (path) => {
      const text = textOf(path);
      if (text === null) return notFound("edit_source", path);
      const markdown = /\.(md|markdown|mdown|mdx)$/i.test(path);
      const known = sources.get(path);
      const segments = known?.text === text ? known.segments : mockSegments(text);
      return ok({
        path,
        text,
        hash: mockHash(text),
        lineEnding: "lf",
        encoding: "utf8",
        segments: markdown ? segments : null,
      });
    },
    editSegments: (text) => ok(mockSegments(text)),
    editSave: (path, seenHash, edits) => {
      const text = textOf(path);
      if (text === null) return ok<SaveOutcome>({ outcome: "conflict", diskHash: null });
      if (mockHash(text) !== seenHash) {
        return ok<SaveOutcome>({ outcome: "conflict", diskHash: mockHash(text) });
      }
      const next = apply(text, edits);
      files.set(path, next);
      probe.saves.push({ path, edits });
      return ok<SaveOutcome>({ outcome: "saved", hash: mockHash(next), mtimeMs: Date.now() });
    },
    editSaveAs: (path, text, exact) => {
      if (exact) {
        files.set(path, text);
        return ok(path);
      }
      const dot = path.lastIndexOf(".");
      const stem = dot > 0 ? path.slice(0, dot) : path;
      const ext = dot > 0 ? path.slice(dot) : "";
      let n = 1;
      let target = `${stem}-mine${ext}`;
      while (files.has(target)) {
        n += 1;
        target = `${stem}-mine-${n}${ext}`;
      }
      files.set(target, text);
      return ok(target);
    },
    editWriteAsset: (doc, ext) => {
      const dot = doc.lastIndexOf(".");
      const stem = dot > 0 ? doc.slice(0, dot) : doc;
      return ok(`${stem}-1.${ext === "jpeg" ? "jpg" : ext}`);
    },
    editSetUnsaved: (path) => {
      probe.unsaved = path;
      return ok(null);
    },
    editHeadings: (path) => {
      const view = mockView(path, [], true);
      return ok(view?.body.kind === "markdown" ? view.body.outline : []);
    },
    editLinkTargets: () => ok(linkTargets()),
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
