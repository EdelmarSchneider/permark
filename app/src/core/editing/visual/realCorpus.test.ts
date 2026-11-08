// @vitest-environment jsdom
// Round-trip audit of Edit mode over real Markdown files (opt-in, never in CI). Input: the JSON lines the Rust audit
// `crates/core/tests/real_corpus.rs` writes (path, editor text, block map). Per file: composed untouched -> must be
// the text byte for byte; up to 5 editable blocks, each edited once per mode (type at the end | at the start, delete
// the last | a middle character, replace a middle word) -> only that block's lines may change, and the block's bytes
// = its own bytes with exactly that change (else "collateral": other bytes rewritten). Fails on damage (untouched,
// outside, missing, error); collateral rewrites are reported by group. Console: counts and groups only (no file text);
// full results with excerpts -> `PERMARK_CORPUS_REPORT` (default: `report.json` next to the input; stays local).
// Run: `PERMARK_CORPUS_JSONL=<file> npx vitest run src/core/editing/visual/realCorpus.test.ts`.
import "./jsdomShims";
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import process from "node:process";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import type { Segment } from "../../../api/generated/Segment";
import { sameMeaning } from "./classify";
import { buildDoc, compose, type Built, type Source } from "./compose";
import { createCore, type Core } from "./core";

const JSONL = process.env.PERMARK_CORPUS_JSONL ?? "";
const REPORT =
  process.env.PERMARK_CORPUS_REPORT ?? (JSONL ? join(dirname(JSONL), "report.json") : "");
/** Files to check at most (0: all). */
const LIMIT = Number(process.env.PERMARK_CORPUS_LIMIT ?? "0");
const EDITS_PER_FILE = 5;
const EDITABLE_TYPES = new Set(["paragraph", "heading", "bullet_list", "ordered_list"]);
const MARK = "Z";
/** Word typed over a word (holds `MARK`: a lost edit is seen). */
const WORD = `${MARK}ork`;

type Mode = "typeEnd" | "typeStart" | "deleteLast" | "deleteMiddle" | "replaceWord";
const MODES: readonly Mode[] = [
  "typeEnd",
  "typeStart",
  "deleteLast",
  "deleteMiddle",
  "replaceWord",
];
/** Document range replaced by `insert`; `remove` = the text it held. */
type Edit = { from: number; to: number; remove: string; insert: string };

type Entry = { path: string; text: string; segments: Segment[] };
type Check =
  | "untouched"
  | "untouchedChanged"
  | "outside"
  | "missing"
  | "count"
  | "collateralMeaning"
  | "collateral"
  | "error";
type Severity = "HIGH" | "MEDIUM" | "LOW";

const SEVERITY: Record<Check, Severity> = {
  untouched: "HIGH",
  untouchedChanged: "MEDIUM",
  outside: "HIGH",
  missing: "MEDIUM",
  count: "MEDIUM",
  collateralMeaning: "MEDIUM",
  collateral: "LOW",
  error: "MEDIUM",
};

type Failure = {
  check: Check;
  /** Edit mode (`load`: the file untouched). */
  mode: Mode | "load";
  path: string;
  /** Block the difference is in (backend kind -> editor node type). */
  kind: string;
  lock: string | null;
  lineStart: number | null;
  /** Expected | written text around the first difference (JSON escaped). */
  expected: string;
  got: string;
  excerpt: string;
  signature: string;
  block: string;
  /** The edited block as written (collateral changes). */
  rewritten?: string;
};

const LOREM = [
  "lorem",
  "ipsum",
  "dolor",
  "sit",
  "amet",
  "elit",
  "sed",
  "tempor",
  "magna",
  "aliqua",
];

/** Keeps the Markdown syntax of `text`, words replaced (tag | entity names, fence info and one-letter words kept). */
function anonymize(text: string): string {
  let k = 0;
  return text
    .split("\n")
    .map((line) => {
      const fence = /^(\s*(?:[-*+>]\s*|\d+[.)]\s*)*(?:`{3,}|~{3,}))(.*)$/.exec(line);
      if (fence) return line;
      return line.replace(
        /(<\/?|&|\[!)?(\p{L}[\p{L}\p{M}'’]*)/gu,
        (all, lead: string | undefined, word: string) => {
          if (lead !== undefined || word.length === 1) return all;
          const lorem = LOREM[k++ % LOREM.length] ?? "lorem";
          const first = word.charAt(0);
          return first === first.toUpperCase() && first !== first.toLowerCase()
            ? lorem.charAt(0).toUpperCase() + lorem.slice(1)
            : lorem;
        },
      );
    })
    .join("\n");
}

function firstDiff(a: string, b: string): number {
  const n = Math.min(a.length, b.length);
  for (let i = 0; i < n; i++) if (a.charCodeAt(i) !== b.charCodeAt(i)) return i;
  return a.length === b.length ? -1 : n;
}

/** Equal characters counted from the end. */
function commonSuffix(a: string, b: string): number {
  let i = 0;
  while (
    i < a.length &&
    i < b.length &&
    a.charCodeAt(a.length - 1 - i) === b.charCodeAt(b.length - 1 - i)
  )
    i++;
  return i;
}

const around = (text: string, at: number): string =>
  JSON.stringify(text.slice(Math.max(0, at - 12), at + 12));
/** Character at `at` for a group key: letters | digits by class only (groups by cause, no file text). */
function charClass(text: string, at: number): string {
  if (at >= text.length) return "EOF";
  const char = text.charAt(at);
  if (/\p{L}/u.test(char)) return "letter";
  if (/\p{N}/u.test(char)) return "digit";
  return JSON.stringify(char);
}

/** Error for a group key: its kind and message up to the first colon (ProseMirror quotes node content after it). */
const errorKey = (e: unknown): string => String(e).split(":").slice(0, 2).join(":").slice(0, 80);

/** Three lines of `text` around offset `at`. */
function excerpt(text: string, at: number): string {
  const lines = text.split("\n");
  let line = 0;
  for (let offset = 0, i = 0; i < lines.length; i++) {
    offset += (lines[i] ?? "").length + 1;
    if (offset > at) {
      line = i;
      break;
    }
    line = i;
  }
  return lines.slice(Math.max(0, line - 1), line + 2).join("\n");
}

/** Block (or separator after a block) holding offset `at` of the text. */
function blockAt(built: Built, at: number): { source: Source | null; index: number } {
  let found: { source: Source | null; index: number } = { source: null, index: -1 };
  built.sources.forEach((source, index) => {
    if (source.start <= at) found = { source, index };
  });
  return found;
}

function failure(
  check: Check,
  mode: Mode | "load",
  path: string,
  source: Source | null,
  expected: string,
  got: string,
  at: number,
  gotAt: number,
): Failure {
  const kind = source ? `${source.kind}->${source.type}` : "separator";
  const lock = source?.locked ?? null;
  const inBlock = source !== null && at >= source.start && at <= source.end;
  const where = source === null ? "before-first" : inBlock ? "block" : "after-block";
  const signature = `${mode}|${check}|${kind}|${String(lock)}|${where}|exp ${charClass(expected, at)} got ${charClass(got, gotAt)}`;
  return {
    check,
    mode,
    path,
    kind,
    lock,
    lineStart: source?.lineStart ?? null,
    expected: around(expected, at),
    got: around(got, gotAt),
    excerpt: excerpt(expected, at),
    signature,
    block: source?.text ?? "",
  };
}

/** Document position at the end of the last text block in the top-level node at `offset`. */
function endOfText(node: PMNode, offset: number): number | null {
  if (node.isTextblock) return offset + node.nodeSize - 1;
  let end: number | null = null;
  node.descendants((child, pos) => {
    if (child.isTextblock) end = offset + 1 + pos + 1 + child.content.size;
    return true;
  });
  return end;
}

/** Document position at the start of the first text block in the top-level node at `offset`. */
function startOfText(node: PMNode, offset: number): number | null {
  if (node.isTextblock) return offset + 1;
  let start: number | null = null;
  node.descendants((child, pos) => {
    if (start === null && child.isTextblock) start = offset + 1 + pos + 1;
    return start === null;
  });
  return start;
}

type TextAt = { pos: number; text: string };

/** Text nodes of the top-level node at `offset`, with their document positions. */
function textsOf(node: PMNode, offset: number): TextAt[] {
  const out: TextAt[] = [];
  node.descendants((child, pos) => {
    if (child.isText) out.push({ pos: offset + 1 + pos, text: child.text ?? "" });
    return true;
  });
  return out;
}

/** Match of the global `pattern` nearest the middle of the node's text (inside one text node). */
function nearMiddle(
  texts: readonly TextAt[],
  pattern: RegExp,
): { pos: number; match: string } | null {
  const total = texts.reduce((n, t) => n + t.text.length, 0);
  let best: { pos: number; match: string; distance: number } | null = null;
  let base = 0;
  for (const { pos, text } of texts) {
    for (const m of text.matchAll(pattern)) {
      const distance = Math.abs(base + m.index - total / 2);
      if (best === null || distance < best.distance) {
        best = { pos: pos + m.index, match: m[0], distance };
      }
    }
    base += text.length;
  }
  return best && { pos: best.pos, match: best.match };
}

/**
 * The edit of `mode` in the top-level node at `offset`. Middle deletions | replacements pick letters | digits: those
 * map 1:1 to the block's bytes (punctuation may carry an escape that goes with it).
 */
function editOf(mode: Mode, node: PMNode, offset: number): Edit | null {
  const insertAt = (at: number | null): Edit | null =>
    at === null ? null : { from: at, to: at, remove: "", insert: MARK };
  const replace = (found: { pos: number; match: string } | null, insert: string): Edit | null =>
    found && { from: found.pos, to: found.pos + found.match.length, remove: found.match, insert };
  switch (mode) {
    case "typeEnd":
      return insertAt(endOfText(node, offset));
    case "typeStart":
      return insertAt(startOfText(node, offset));
    case "deleteLast": {
      const last = textsOf(node, offset).at(-1);
      const char = last ? Array.from(last.text).at(-1) : undefined;
      if (!last || char === undefined) return null;
      return replace({ pos: last.pos + last.text.length - char.length, match: char }, "");
    }
    case "deleteMiddle":
      return replace(nearMiddle(textsOf(node, offset), /[\p{L}\p{N}]/gu), "");
    case "replaceWord":
      return replace(nearMiddle(textsOf(node, offset), /\p{L}{2,}/gu), WORD);
    default:
      return mode satisfies never;
  }
}

/** `got` = `original` with one `remove` -> `insert` and nothing else. */
function oneChange(original: string, got: string, remove: string, insert: string): boolean {
  if (got.length !== original.length - remove.length + insert.length) return false;
  const p = firstDiff(original, got);
  if (p < 0) return remove === insert;
  const q = commonSuffix(original, got);
  for (let x = Math.max(0, original.length - remove.length - q); x <= p; x++) {
    if (
      original.startsWith(remove, x) &&
      got.startsWith(insert, x) &&
      original.slice(x + remove.length) === got.slice(x + insert.length)
    ) {
      return true;
    }
  }
  return false;
}

/** The block's new bytes = its own bytes with exactly the edit (a deleted character takes its escape along). */
const exactly = (original: string, got: string, edit: Edit): boolean =>
  oneChange(original, got, edit.remove, edit.insert) ||
  (edit.insert === "" && oneChange(original, got, `\\${edit.remove}`, ""));

/** Up to `count` indexes spread over `items`. */
function spread<T>(items: readonly T[], count: number): T[] {
  if (items.length <= count) return [...items];
  const picked = new Set(
    Array.from({ length: count }, (_, i) => Math.floor((i * items.length) / count)),
  );
  return items.filter((_, i) => picked.has(i));
}

function loadEntries(): Entry[] {
  const lines = readFileSync(JSONL, "utf8")
    .split("\n")
    .filter((l) => l.trim() !== "");
  const all = lines.map((l) => JSON.parse(l) as Entry);
  return LIMIT > 0 ? all.slice(0, LIMIT) : all;
}

let core: Core;

describe.skipIf(!JSONL)("Edit mode over a real corpus", () => {
  beforeAll(async () => {
    const host = document.createElement("div");
    document.body.append(host);
    core = await createCore({ host });
  });

  afterAll(async () => {
    await core.destroy();
  });

  it("keeps untouched files and the blocks around an edit byte for byte", async () => {
    const started = performance.now();
    const entries = loadEntries();
    const failures: Failure[] = [];
    const locks = new Map<string, number>();
    const kinds = new Map<string, { blocks: number; locked: number }>();
    let blocks = 0;
    let edits = 0;
    let cleanEdits = 0;
    let allLockedFiles = 0;
    const slow: { path: string; ms: number }[] = [];
    const byMode: Record<Mode, { edits: number; clean: number }> = {
      typeEnd: { edits: 0, clean: 0 },
      typeStart: { edits: 0, clean: 0 },
      deleteLast: { edits: 0, clean: 0 },
      deleteMiddle: { edits: 0, clean: 0 },
      replaceWord: { edits: 0, clean: 0 },
    };

    for (const { path, text, segments } of entries) {
      const fileStart = performance.now();
      let built: Built;
      try {
        built = await buildDoc(core, text, segments);
        core.setDoc(built.doc);
      } catch (e) {
        failures.push({
          ...failure("error", "load", path, null, text, "", 0, 0),
          got: String(e),
          signature: `load|error|build|${errorKey(e)}`,
        });
        continue;
      }
      // coverage
      for (const s of built.sources) {
        blocks += 1;
        const reason = s.locked ?? "editable";
        locks.set(reason, (locks.get(reason) ?? 0) + 1);
        const k = kinds.get(s.kind) ?? { blocks: 0, locked: 0 };
        k.blocks += 1;
        if (s.locked !== null) k.locked += 1;
        kinds.set(s.kind, k);
      }
      if (built.sources.length > 0 && built.sources.every((s) => s.locked !== null))
        allLockedFiles += 1;

      // 1. untouched
      const composed = compose(core, built, core.view.state.doc);
      if (composed.text !== text) {
        const at = firstDiff(text, composed.text);
        failures.push(
          failure(
            "untouched",
            "load",
            path,
            blockAt(built, at).source,
            text,
            composed.text,
            at,
            at,
          ),
        );
      } else if (composed.changed !== 0) {
        failures.push({
          ...failure("untouchedChanged", "load", path, null, text, composed.text, 0, 0),
          got: `changed=${composed.changed}`,
        });
      }

      // 2. one block edited, once per mode
      const offsets: number[] = [];
      built.doc.forEach((_, offset) => offsets.push(offset));
      const candidates = built.sources
        .map((s, i) => ({ s, i }))
        .filter(({ s }) => s.locked === null && EDITABLE_TYPES.has(s.type));
      for (const { s: source, i } of spread(candidates, EDITS_PER_FILE)) {
        for (const mode of MODES) {
          core.setDoc(built.doc);
          const edit = editOf(mode, built.doc.child(i), offsets[i] ?? 0);
          if (edit === null) continue;
          const tally = byMode[mode];
          tally.edits += 1;
          edits += 1;
          let written: string;
          let changed: number;
          let node: PMNode;
          try {
            const view = core.view;
            // a plain range delete (as Backspace inside text): `insertText("")` may take whole nodes along; a
            // replaced word keeps its marks (`insertText` drops a code | link mark ending with it): 1:1 with the bytes
            const marks = view.state.doc.nodeAt(edit.from)?.marks ?? [];
            const tr =
              edit.insert === ""
                ? view.state.tr.delete(edit.from, edit.to)
                : edit.from === edit.to
                  ? view.state.tr.insertText(edit.insert, edit.from)
                  : view.state.tr.replaceWith(
                      edit.from,
                      edit.to,
                      view.state.schema.text(edit.insert, marks),
                    );
            view.dispatch(tr);
            node = view.state.doc.child(i);
            const result = compose(core, built, view.state.doc);
            written = result.text;
            changed = result.changed;
          } catch (e) {
            failures.push({
              ...failure("error", mode, path, source, text, "", source.start, 0),
              got: String(e),
              signature: `${mode}|error|edit|${source.kind}->${source.type}|${errorKey(e)}`,
            });
            continue;
          }
          const before = text.slice(0, source.start);
          const after = text.slice(source.end);
          const head = firstDiff(before, written.slice(0, before.length));
          const tail = commonSuffix(after, written);
          if (head >= 0 || tail < after.length || written.length < before.length + after.length) {
            if (head >= 0) {
              failures.push(
                failure(
                  "outside",
                  mode,
                  path,
                  blockAt(built, head).source,
                  text,
                  written,
                  head,
                  head,
                ),
              );
            } else {
              // first differing character of the suffix, counted from its start
              const at = text.length - tail - 1;
              const gotAt = written.length - tail - 1;
              failures.push(
                failure("outside", mode, path, blockAt(built, at).source, text, written, at, gotAt),
              );
            }
            continue;
          }
          const middle = written.slice(before.length, written.length - after.length);
          // the edit is lost: block unchanged, or fewer `MARK`s than it adds (a replaced word may hold one)
          const count = (t: string): number => t.split(MARK).length - 1;
          if (
            middle === source.text ||
            count(middle) - count(source.text) < count(edit.insert) - count(edit.remove)
          ) {
            failures.push(
              failure("missing", mode, path, source, text, written, source.start, source.start),
            );
            continue;
          }
          if (changed !== 1) {
            failures.push({
              ...failure("count", mode, path, source, text, written, source.start, source.start),
              got: `changed=${changed}`,
            });
          }
          if (exactly(source.text, middle, edit)) {
            tally.clean += 1;
            cleanEdits += 1;
          } else {
            const d = Math.max(0, firstDiff(source.text, middle));
            failures.push({
              // rewritten bytes but the same document as the editor's: noise, not damage
              ...failure(
                sameMeaning(core, middle, core.write(node)) ? "collateral" : "collateralMeaning",
                mode,
                path,
                source,
                text,
                written,
                source.start + d,
                before.length + d,
              ),
              rewritten: middle,
            });
          }
        }
      }
      const ms = performance.now() - fileStart;
      if (ms > 2000) slow.push({ path, ms: Math.round(ms) });
    }

    // grouping
    const groups = new Map<string, Failure[]>();
    for (const f of failures) {
      const list = groups.get(f.signature) ?? [];
      list.push(f);
      groups.set(f.signature, list);
    }
    const ranked = [...groups.entries()]
      .flatMap(([signature, list]) => {
        const first = list[0];
        if (!first) return [];
        const repro = anonymize(first.block.split("\n").slice(0, 12).join("\n"));
        const group = {
          signature,
          check: first.check,
          severity: SEVERITY[first.check],
          count: list.length,
          files: [...new Set(list.map((f) => f.path))].length,
          examples: list.slice(0, 5).map((f) => ({
            path: f.path,
            line: f.lineStart,
            expected: f.expected,
            got: f.got,
            excerpt: f.excerpt,
          })),
          repro,
        };
        return [group];
      })
      .sort((a, b) => {
        const rank = { HIGH: 0, MEDIUM: 1, LOW: 2 };
        return rank[a.severity] - rank[b.severity] || b.count - a.count;
      });
    const pct = (n: number) => Math.round((1000 * n) / Math.max(1, blocks)) / 10;
    const byCheck = (list: readonly Failure[]): Record<Check, number> => {
      const counts: Record<Check, number> = {
        untouched: 0,
        untouchedChanged: 0,
        outside: 0,
        missing: 0,
        count: 0,
        collateralMeaning: 0,
        collateral: 0,
        error: 0,
      };
      for (const f of list) counts[f.check] += 1;
      return counts;
    };
    const failuresByCheck = byCheck(failures);
    const report = {
      files: entries.length,
      blocks,
      edits,
      cleanEdits,
      allLockedFiles,
      durationMs: Math.round(performance.now() - started),
      failuresByCheck,
      byMode: Object.fromEntries(
        MODES.map((m) => [m, { ...byMode[m], ...byCheck(failures.filter((f) => f.mode === m)) }]),
      ),
      lockReasons: Object.fromEntries(
        [...locks.entries()]
          .sort((a, b) => b[1] - a[1])
          .map(([r, n]) => [r, { count: n, pct: pct(n) }]),
      ),
      kinds: Object.fromEntries([...kinds.entries()].sort((a, b) => b[1].blocks - a[1].blocks)),
      slow,
      groups: ranked,
      failures,
    };
    writeFileSync(REPORT, JSON.stringify(report, null, 2));
    // counts and groups only: file text (excerpts, paths) stays in the local report file
    const summary = {
      files: report.files,
      blocks,
      edits,
      cleanEdits,
      allLockedFiles,
      durationMs: report.durationMs,
      failuresByCheck,
      byMode: report.byMode,
      lockReasons: report.lockReasons,
      slowFiles: slow.length,
      groups: ranked.map((g) => ({ signature: g.signature, count: g.count, files: g.files })),
    };
    console.log(JSON.stringify(summary, null, 2));
    expect(entries.length).toBeGreaterThan(0);
    const damage =
      failuresByCheck.untouched +
      failuresByCheck.outside +
      failuresByCheck.missing +
      failuresByCheck.error;
    expect(damage).toBe(0);
  }, 3_600_000);
});
