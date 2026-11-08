// @vitest-environment jsdom
// Round-trip audit of Edit mode over real Markdown files (opt-in, never in CI). Input: the JSON lines the Rust audit
// `crates/core/tests/real_corpus.rs` writes (path, editor text, block map). Per file: composed untouched -> must be
// the text byte for byte; one "Z" typed at the end of up to 5 editable blocks -> only that block's lines may change.
// Failures grouped by cause; full results -> `PERMARK_CORPUS_REPORT` (default: `report.json` next to the input).
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

const CHECKS: readonly Check[] = [
  "untouched",
  "untouchedChanged",
  "outside",
  "missing",
  "count",
  "collateralMeaning",
  "collateral",
  "error",
];
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
const charAt = (text: string, at: number): string =>
  at < text.length ? JSON.stringify(text.charAt(at)) : "EOF";

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
  const signature = `${check}|${kind}|${String(lock)}|${where}|exp ${charAt(expected, at)} got ${charAt(got, gotAt)}`;
  return {
    check,
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

    for (const { path, text, segments } of entries) {
      const fileStart = performance.now();
      let built: Built;
      try {
        built = await buildDoc(core, text, segments);
        core.setDoc(built.doc);
      } catch (e) {
        failures.push({
          ...failure("error", path, null, text, "", 0, 0),
          got: String(e),
          signature: `error|build|${String(e).slice(0, 80)}`,
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
          failure("untouched", path, blockAt(built, at).source, text, composed.text, at, at),
        );
      } else if (composed.changed !== 0) {
        failures.push({
          ...failure("untouchedChanged", path, null, text, composed.text, 0, 0),
          got: `changed=${composed.changed}`,
        });
      }

      // 2. one block edited
      const offsets: number[] = [];
      built.doc.forEach((_, offset) => offsets.push(offset));
      const candidates = built.sources
        .map((s, i) => ({ s, i }))
        .filter(({ s }) => s.locked === null && EDITABLE_TYPES.has(s.type));
      for (const { s: source, i } of spread(candidates, EDITS_PER_FILE)) {
        core.setDoc(built.doc);
        const node = built.doc.child(i);
        const at = endOfText(node, offsets[i] ?? 0);
        if (at === null) continue;
        edits += 1;
        let written: string;
        let changed: number;
        try {
          core.view.dispatch(core.view.state.tr.insertText(MARK, at));
          const result = compose(core, built, core.view.state.doc);
          written = result.text;
          changed = result.changed;
        } catch (e) {
          failures.push({
            ...failure("error", path, source, text, "", source.start, 0),
            got: String(e),
            signature: `error|edit|${source.kind}->${source.type}|${String(e).slice(0, 80)}`,
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
              failure("outside", path, blockAt(built, head).source, text, written, head, head),
            );
          } else {
            // first differing character of the suffix, counted from its start
            const at = text.length - tail - 1;
            const gotAt = written.length - tail - 1;
            failures.push(
              failure("outside", path, blockAt(built, at).source, text, written, at, gotAt),
            );
          }
          continue;
        }
        const middle = written.slice(before.length, written.length - after.length);
        if (!middle.includes(MARK) || middle.split(MARK).length <= source.text.split(MARK).length) {
          failures.push(
            failure("missing", path, source, text, written, source.start, source.start),
          );
          continue;
        }
        if (changed !== 1) {
          failures.push({
            ...failure("count", path, source, text, written, source.start, source.start),
            got: `changed=${changed}`,
          });
        }
        // the block itself: only the typed character added
        const p = firstDiff(source.text, middle);
        const q = commonSuffix(source.text, middle);
        const surgical =
          middle.length === source.text.length + 1 &&
          p + q >= source.text.length &&
          middle.charAt(p) === MARK;
        if (surgical) cleanEdits += 1;
        else {
          const d = Math.max(0, p);
          failures.push({
            // rewritten bytes but the same document as the block with the mark at its end: noise, not damage
            ...failure(
              sameMeaning(core, `${source.text.trimEnd()}${MARK}`, middle)
                ? "collateral"
                : "collateralMeaning",
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
    const report = {
      files: entries.length,
      blocks,
      edits,
      cleanEdits,
      allLockedFiles,
      durationMs: Math.round(performance.now() - started),
      failuresByCheck: Object.fromEntries(
        CHECKS.map((c) => [c, failures.filter((f) => f.check === c).length]),
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
    const summary = {
      ...report,
      groups: ranked.length,
      failures: failures.length,
      kinds: undefined,
    };
    console.log(JSON.stringify(summary, null, 2));
    expect(entries.length).toBeGreaterThan(0);
  }, 3_600_000);
});
