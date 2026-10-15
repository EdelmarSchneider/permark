// File operations for the browser mock (`VITE_MOCK=1`): rename, move, delete and new folders over the mock
// documents. Only Markdown documents can move (the mock keeps no bytes for the rest), links are found with a plain
// text search, and a "recycle bin" is just the deleted set. Enough to try the Explorer end to end without Rust.

import { dirname, basename } from "../paths";
import { isInside } from "../fileOps";
import { noteGone, noteMoved } from "./mockUnread";
import type {
  BufferText,
  CreatedFolder,
  DeleteOutcome,
  Move,
  MoveOutcome,
  MovePreview,
  PreviewChange,
  PreviewFile,
  SkippedFile,
  TextEdit,
  UndoOutcome,
} from "../types";
import {
  allDocPaths,
  docExists,
  mockDocKind,
  mockFiles,
  mockState,
  sourceText,
  writeSource,
} from "./fixtures";

type Plan = {
  moves: Move[];
  files: { path: string; movedTo: string | null; edits: Edit[] }[];
  skipped: SkippedFile[];
};
type Edit = { old: string; new: string; at: number; line: number };
type Operation = {
  id: number;
  moves: Move[];
  rewrites: { path: string; before: string }[];
  folders: string[];
};

const plans = new Map<number, Plan>();
const operations: Operation[] = [];
let counter = 0;

/** What the backend adds to an error for the UI to word (see `errorMessage`). */
type Blanks = { subject?: string; detail?: string; reason?: string };

const fail = (kind: string, message: string, blanks: Blanks = {}): never => {
  throw { kind, message, ...blanks };
};

function relative(fromDir: string, target: string): string {
  const from = fromDir.split("/").filter(Boolean);
  const to = target.split("/").filter(Boolean);
  let common = 0;
  while (common < from.length && common < to.length - 1 && from[common] === to[common]) common++;
  return [...Array<string>(from.length - common).fill(".."), ...to.slice(common)].join("/");
}

/** The documents a move touches: a document itself, or every document of a folder. */
function docsOf(move: Move): Move[] {
  return allDocPaths()
    .filter((p) => isInside(p, move.from))
    .map((p) => ({ from: p, to: move.to + p.slice(move.from.length) }));
}

function check(moves: Move[]): Move[] {
  if (moves.length === 0) return fail("nothing", "there is nothing to do");
  const out: Move[] = [];
  for (const m of moves) {
    if (m.from.startsWith("@wt/") !== m.to.startsWith("@wt/")) {
      fail(
        "otherRoot",
        `${m.to} is in another root: files can't be moved between the project and a worktree`,
        { subject: m.to },
      );
    }
    if (isInside(m.to, m.from))
      fail("intoItself", `${m.from} can't be moved into itself`, { subject: m.from });
    if (docExists(m.to) || allDocPaths().some((p) => isInside(p, m.to))) {
      fail("exists", `${m.to} already exists`, { subject: m.to });
    }
    const docs = docsOf(m);
    if (docs.length === 0 && !mockFiles.folders.has(m.from) && mockDocKind(m.from) === null) {
      fail("io", "The mock can only move Markdown documents and folders made in this session.");
    }
    out.push(m);
  }
  return out;
}

function scan(moves: Move[]): Plan["files"] {
  const docMoves = moves.flatMap(docsOf);
  const mapped = (p: string): string => docMoves.find((m) => m.from === p)?.to ?? p;
  const files: Plan["files"] = [];
  for (const doc of allDocPaths()) {
    const text = sourceText(doc);
    if (text === null) continue;
    const edits: Edit[] = [];
    for (const m of docMoves) {
      const oldRel = relative(dirname(doc), m.from);
      const newRel = relative(dirname(mapped(doc)), m.to);
      if (oldRel === newRel) continue;
      const pattern = new RegExp(
        `\\]\\((${oldRel.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")})(#[^)\\s]*)?\\)`,
        "g",
      );
      for (const hit of text.matchAll(pattern)) {
        const at = (hit.index ?? 0) + 2;
        edits.push({
          old: hit[1] + (hit[2] ?? ""),
          new: newRel + (hit[2] ?? ""),
          at,
          line: text.slice(0, at).split("\n").length,
        });
      }
    }
    if (edits.length > 0) {
      files.push({ path: doc, movedTo: mapped(doc) === doc ? null : mapped(doc), edits });
    }
  }
  return files;
}

export async function previewMove(
  moves: Move[],
  scanLinks: boolean,
  heldBack: string[],
): Promise<MovePreview> {
  const checked = check(moves);
  const found = scanLinks ? scan(checked) : [];
  // documents with unsaved edits in Visual mode are left out and reported
  const files = found.filter((f) => !heldBack.includes(f.path));
  const skipped = found
    .filter((f) => heldBack.includes(f.path))
    .map((f): SkippedFile => ({ path: f.path, reason: "unsavedVisual" }));
  const planId = ++counter;
  plans.set(planId, { moves: checked, files, skipped });
  const shown: PreviewFile[] = files.map((f) => ({
    path: f.path,
    movedTo: f.movedTo,
    changes: f.edits.map((e): PreviewChange => ({ line: e.line, old: e.old, new: e.new })),
  }));
  return {
    planId,
    moves: checked,
    linkCount: files.reduce((n, f) => n + f.edits.length, 0),
    fileCount: files.length,
    files: shown,
    truncated: false,
    skipped,
  };
}

function applyEdits(text: string, edits: Edit[]): string {
  let out = text;
  for (const e of [...edits].sort((a, b) => b.at - a.at)) {
    out = out.slice(0, e.at) + e.new + out.slice(e.at + e.old.length);
  }
  return out;
}

function toUtf16(edits: Edit[]): TextEdit[] {
  return edits.map((e) => ({ from: e.at, to: e.at + e.old.length, insert: e.new }));
}

export async function applyMove(
  planId: number,
  updateLinks: boolean,
  buffers: BufferText[],
): Promise<MoveOutcome> {
  const plan = plans.get(planId);
  if (!plan) return fail("unknownPlan", "that preview is no longer available; try again");
  plans.delete(planId);
  const inBuffers = new Set(buffers.map((b) => b.path));
  const docMoves = plan.moves.flatMap(docsOf);
  const rewrites: Operation["rewrites"] = [];
  const bufferEdits: MoveOutcome["buffers"] = [];

  if (updateLinks) {
    for (const b of buffers) {
      const mine = plan.files.find((f) => f.path === b.path);
      if (mine) {
        bufferEdits.push({
          path: b.path,
          newPath: mine.movedTo ?? b.path,
          edits: toUtf16(mine.edits),
        });
      }
    }
  }
  const texts = new Map(allDocPaths().map((p) => [p, sourceText(p) ?? ""]));
  for (const m of docMoves) {
    mockFiles.created.add(m.to);
    writeSource(m.to, texts.get(m.from) ?? "");
    mockState.deleted.add(m.from);
    mockState.deleted.delete(m.to);
  }
  for (const m of plan.moves) {
    if (mockFiles.folders.has(m.from)) {
      mockFiles.folders.delete(m.from);
      mockFiles.folders.add(m.to);
    }
  }
  let links = 0;
  if (updateLinks) {
    for (const f of plan.files) {
      if (inBuffers.has(f.path)) continue;
      const path = f.movedTo ?? f.path;
      const before = texts.get(f.path) ?? "";
      writeSource(path, applyEdits(before, f.edits));
      rewrites.push({ path, before });
      links += f.edits.length;
    }
  }
  noteMoved(plan.moves);
  const id = ++counter;
  operations.push({ id, moves: plan.moves, rewrites, folders: [] });
  return {
    opId: id,
    moved: plan.moves,
    filesUpdated: rewrites.length,
    linksUpdated: links,
    buffers: bufferEdits,
    skipped: plan.skipped,
  };
}

export async function deleteEntries(paths: string[]): Promise<DeleteOutcome> {
  if (paths.length === 0) return fail("nothing", "there is nothing to do");
  for (const path of paths) {
    for (const doc of allDocPaths().filter((p) => isInside(p, path))) mockState.deleted.add(doc);
    mockFiles.folders.delete(path);
    noteGone(path);
  }
  return { deleted: paths };
}

export async function newFolder(path: string): Promise<CreatedFolder> {
  if (allDocPaths().some((p) => isInside(p, path)) || mockFiles.folders.has(path)) {
    return fail("exists", `${path} already exists`, { subject: path });
  }
  if (/[\\:*?"<>|]/.test(basename(path)))
    return fail("invalidName", "That name can't be used", { reason: "forbiddenChars" });
  mockFiles.folders.add(path);
  const id = ++counter;
  operations.push({ id, moves: [], rewrites: [], folders: [path] });
  return { opId: id, path };
}

export async function undoFileOp(opId: number): Promise<UndoOutcome> {
  const at = operations.findIndex((o) => o.id === opId);
  if (at < 0) return fail("unknownOperation", "that operation can no longer be undone");
  const [op] = operations.splice(at, 1);
  for (const r of op.rewrites) writeSource(r.path, r.before);
  const back = [...op.moves].reverse().map((m) => ({ from: m.to, to: m.from }));
  for (const m of back) {
    for (const d of allDocPaths().filter((p) => isInside(p, m.from))) {
      const target = m.to + d.slice(m.from.length);
      mockFiles.created.add(target);
      writeSource(target, sourceText(d) ?? "");
      mockState.deleted.add(d);
      mockState.deleted.delete(target);
    }
    if (mockFiles.folders.has(m.from)) {
      mockFiles.folders.delete(m.from);
      mockFiles.folders.add(m.to);
    }
  }
  noteMoved(back);
  let removed = 0;
  for (const f of op.folders) removed += mockFiles.folders.delete(f) ? 1 : 0;
  return {
    movedBack: back,
    filesRestored: op.rewrites.length,
    linksRestored: 0,
    foldersRemoved: removed,
    couldNot: [],
  };
}
