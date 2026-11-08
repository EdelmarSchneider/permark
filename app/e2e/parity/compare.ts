// View vs Edit comparison of one document (pure; tested). Blocks pair by kind + text, elements pair by order within
// their type inside a block pair: the two modes build different DOM (wrappers, `li > p`), so exact DOM matching fails.

export type Theme = "light" | "dark";
export type Props = Record<string, string>;

/** One element of a block: `type` pairs elements across modes, `label` adds what this side marks it as. */
export type Item = { type: string; label: string; props: Props };

export type Block = {
  /** `h1` | `p` | `ul` | `code` | `table` | `locked` | ... */
  kind: string;
  /** Kind + text: equal keys = the same block in both modes. */
  key: string;
  /** Border box, relative to the content root. */
  top: number;
  bottom: number;
  props: Props;
  items: Item[];
  /** Edit only: a block the visual editor shows as source. */
  locked: { kind: string; reason: string } | null;
};

export type Side = { column: Props; blocks: Block[] };

/** What entering Edit did: scroll offsets before | after, selected nodes, rows of the toolbar. */
export type Entry = {
  viewScroll: number;
  editScroll: number;
  selected: number;
  toolbarRows: number;
};

export type Diff = {
  doc: string;
  theme: Theme;
  element: string;
  index: number;
  property: string;
  view: string;
  edit: string;
};

type Where = { doc: string; theme: Theme };

const LENGTH = /^(-?\d+(?:\.\d+)?)px$/;
/** Entering Edit may land a fraction off the reading place (rounding of the scroll offset). */
const SCROLL_TOLERANCE = 2;

export function px(n: number): string {
  return `${Math.round(n * 100) / 100}px`;
}

/** Equal computed values: lengths within `tolerance` px, every other token exact. */
export function same(a: string, b: string, tolerance = 1): boolean {
  if (a === b) return true;
  const ta = a.split(/[\s,/]+/);
  const tb = b.split(/[\s,/]+/);
  if (ta.length !== tb.length) return false;
  return ta.every((x, i) => {
    const y = tb[i] ?? "";
    if (x === y) return true;
    const mx = LENGTH.exec(x);
    const my = LENGTH.exec(y);
    return mx !== null && my !== null && Math.abs(Number(mx[1]) - Number(my[1])) <= tolerance;
  });
}

/** Pairs `[view, edit]` of block indexes with equal keys (longest common subsequence, document order). */
export function align(view: Block[], edit: Block[]): [number, number][] {
  const n = view.length;
  const m = edit.length;
  const len: number[][] = Array.from({ length: n + 1 }, () => new Array<number>(m + 1).fill(0));
  const at = (i: number, j: number): number => len[i]?.[j] ?? 0;
  for (let i = n - 1; i >= 0; i--) {
    const row = len[i];
    if (row === undefined) continue;
    for (let j = m - 1; j >= 0; j--) {
      const v = view[i];
      const e = edit[j];
      const pairs = v !== undefined && e !== undefined && e.locked === null && v.key === e.key;
      row[j] = pairs ? at(i + 1, j + 1) + 1 : Math.max(at(i + 1, j), at(i, j + 1));
    }
  }
  const out: [number, number][] = [];
  let i = 0;
  let j = 0;
  while (i < n && j < m) {
    const v = view[i];
    const e = edit[j];
    if (v !== undefined && e !== undefined && e.locked === null && v.key === e.key) {
      out.push([i, j]);
      i++;
      j++;
    } else if (at(i + 1, j) >= at(i, j + 1)) i++;
    else j++;
  }
  return out;
}

function propDiffs(where: Where, element: string, index: number, view: Props, edit: Props): Diff[] {
  const out: Diff[] = [];
  for (const property of new Set([...Object.keys(view), ...Object.keys(edit)])) {
    const v = view[property] ?? "";
    const e = edit[property] ?? "";
    if (!same(v, e)) out.push({ ...where, element, index, property, view: v, edit: e });
  }
  return out;
}

function byType(items: Item[]): Map<string, Item[]> {
  const out = new Map<string, Item[]>();
  for (const item of items) {
    const list = out.get(item.type) ?? [];
    list.push(item);
    out.set(item.type, list);
  }
  return out;
}

/** Blocks between two pairs: locked ones explain the gap (one row each); with none, every unpaired block is a row. */
function unpaired(
  where: Where,
  view: Block[],
  edit: Block[],
  counters: Map<string, number>,
): Diff[] {
  const next = (key: string): number => {
    const n = (counters.get(key) ?? 0) + 1;
    counters.set(key, n);
    return n;
  };
  const locked = edit.filter((b) => b.locked !== null);
  if (locked.length > 0) {
    return locked.map((b) => {
      const element = `locked(${b.locked?.kind ?? "other"})`;
      return {
        ...where,
        element,
        index: next(element),
        property: "reason",
        view: "-",
        edit: b.locked?.reason ?? "",
      };
    });
  }
  const row = (b: Block, inView: boolean): Diff => {
    const element = `block(${b.kind})`;
    return {
      ...where,
      element,
      index: next(element),
      property: "only-in",
      view: inView ? "yes" : "no",
      edit: inView ? "no" : "yes",
    };
  };
  return [...view.map((b) => row(b, true)), ...edit.map((b) => row(b, false))];
}

/** Every difference between the two modes of one document. */
export function compareSides(where: Where, view: Side, edit: Side): Diff[] {
  const out = propDiffs(where, "column", 1, view.column, edit.column);
  const counters = new Map<string, number>();
  const next = (key: string, by = 1): number => {
    const n = (counters.get(key) ?? 0) + by;
    counters.set(key, n);
    return n;
  };
  const pairs = align(view.blocks, edit.blocks);
  // past the last pair: the blocks after it are unpaired too
  const end: [number, number] = [view.blocks.length, edit.blocks.length];
  let vi = 0;
  let ei = 0;
  for (const [k, [pv, pe]] of [...pairs, end].entries()) {
    out.push(...unpaired(where, view.blocks.slice(vi, pv), edit.blocks.slice(ei, pe), counters));
    vi = pv + 1;
    ei = pe + 1;
    const v = view.blocks[pv];
    const e = edit.blocks[pe];
    if (v === undefined || e === undefined) continue;
    const element = `block(${v.kind})`;
    const index = next(element);
    out.push(...propDiffs(where, element, index, v.props, e.props));
    const following = pairs[k + 1];
    if (following !== undefined && following[0] === pv + 1 && following[1] === pe + 1) {
      const vn = view.blocks[pv + 1];
      const en = edit.blocks[pe + 1];
      if (vn !== undefined && en !== undefined) {
        const gv = px(vn.top - v.bottom);
        const ge = px(en.top - e.bottom);
        if (!same(gv, ge)) {
          out.push({ ...where, element, index, property: "gap-after", view: gv, edit: ge });
        }
      }
    }
    const vt = byType(v.items);
    const et = byType(e.items);
    for (const type of new Set([...vt.keys(), ...et.keys()])) {
      const vs = vt.get(type) ?? [];
      const es = et.get(type) ?? [];
      const both = Math.min(vs.length, es.length);
      for (let i = 0; i < both; i++) {
        const a = vs[i];
        const b = es[i];
        if (a === undefined || b === undefined) continue;
        out.push(...propDiffs(where, a.label, next(type), a.props, b.props));
      }
      if (vs.length !== es.length) {
        const label = (vs[0] ?? es[0])?.label ?? type;
        out.push({
          ...where,
          element: label,
          index: (counters.get(type) ?? 0) + 1,
          property: "count",
          view: String(vs.length),
          edit: String(es.length),
        });
        next(type, Math.max(vs.length, es.length) - both);
      }
    }
  }
  return out;
}

/** Entering Edit keeps the reading place, selects nothing and shows the toolbar on one row. */
export function entryDiffs(where: Where, entry: Entry): Diff[] {
  const out: Diff[] = [];
  const row = (property: string, view: string, edit: string): Diff => ({
    ...where,
    element: "entry",
    index: 1,
    property,
    view,
    edit,
  });
  if (entry.selected !== 0) out.push(row("selected-node", "0", String(entry.selected)));
  const sv = px(entry.viewScroll);
  const se = px(entry.editScroll);
  if (!same(sv, se, SCROLL_TOLERANCE)) out.push(row("scroll-top", sv, se));
  if (entry.toolbarRows > 1) out.push(row("toolbar-rows", "1", String(entry.toolbarRows)));
  return out;
}

/** The later parity work each known gap waits for. */
export const PHASES = [
  "code-colors",
  "locked-blocks",
  "fewer-locks",
  "inline-decorations",
] as const;
export type Phase = (typeof PHASES)[number];

/** A difference known and planned: `element` | `property` | `doc` match with `*` as wildcard. */
export type KnownGap = {
  phase: Phase;
  element: string;
  property: string;
  doc: string;
  why: string;
};

function text(v: unknown, what: string, def?: string): string {
  if (v === undefined && def !== undefined) return def;
  if (typeof v !== "string" || v === "") throw new Error(`known gaps: ${what} text expected`);
  return v;
}

function isPhase(v: unknown): v is Phase {
  return PHASES.some((p) => p === v);
}

export function parseKnownGaps(raw: unknown): KnownGap[] {
  const gaps: unknown =
    typeof raw === "object" && raw !== null && "gaps" in raw ? raw.gaps : undefined;
  if (!Array.isArray(gaps)) throw new Error("known gaps: { gaps: [...] } expected");
  return gaps.map((g: unknown, i) => {
    if (typeof g !== "object" || g === null)
      throw new Error(`known gaps: entry ${i} not an object`);
    const entry: Record<string, unknown> = { ...g };
    if (!isPhase(entry["phase"])) {
      throw new Error(`known gaps: entry ${i} phase one of ${PHASES.join(", ")}`);
    }
    return {
      phase: entry["phase"],
      element: text(entry["element"], `entry ${i} element`),
      property: text(entry["property"], `entry ${i} property`, "*"),
      doc: text(entry["doc"], `entry ${i} doc`, "*"),
      why: text(entry["why"], `entry ${i} why`),
    };
  });
}

function matches(pattern: string, value: string): boolean {
  if (pattern === "*") return true;
  const source = pattern
    .split("*")
    .map((part) => part.replace(/[.*+?^${}()|[\]\\]/g, "\\$&"))
    .join(".*");
  return new RegExp(`^${source}$`).test(value);
}

function covers(gap: KnownGap, diff: Diff): boolean {
  return (
    matches(gap.element, diff.element) &&
    matches(gap.property, diff.property) &&
    matches(gap.doc, diff.doc)
  );
}

export type Triage = {
  uncovered: Diff[];
  covered: { diff: Diff; gap: KnownGap }[];
  /** Known gaps that no longer occur: remove them from the list. */
  stale: KnownGap[];
};

export function triage(diffs: Diff[], gaps: KnownGap[]): Triage {
  const hit = new Set<KnownGap>();
  const out: Triage = { uncovered: [], covered: [], stale: [] };
  for (const diff of diffs) {
    const found = gaps.filter((g) => covers(g, diff));
    for (const g of found) hit.add(g);
    const first = found[0];
    if (first === undefined) out.uncovered.push(diff);
    else out.covered.push({ diff, gap: first });
  }
  out.stale = gaps.filter((g) => !hit.has(g));
  return out;
}

export type DiffGroup = {
  doc: string;
  element: string;
  property: string;
  /** The first occurrence's values. */
  view: string;
  edit: string;
  /** Edit minus View for a single length. */
  delta: number | null;
  themes: Theme[];
  indexes: number[];
  count: number;
};

function deltaOf(view: string, edit: string): number | null {
  const v = LENGTH.exec(view);
  const e = LENGTH.exec(edit);
  if (v === null || e === null) return null;
  return Math.round((Number(e[1]) - Number(v[1])) * 2) / 2;
}

/** One row per document, element type, property and kind of difference (same values | same length change). */
export function groupDiffs(diffs: Diff[]): DiffGroup[] {
  const groups = new Map<string, DiffGroup>();
  for (const d of diffs) {
    const delta = deltaOf(d.view, d.edit);
    const what = delta === null ? `${d.view}\u0000${d.edit}` : `d${delta}`;
    const key = [d.doc, d.element, d.property, what].join("\u0001");
    const group = groups.get(key) ?? {
      doc: d.doc,
      element: d.element,
      property: d.property,
      view: d.view,
      edit: d.edit,
      delta,
      themes: [],
      indexes: [],
      count: 0,
    };
    if (!group.themes.includes(d.theme)) group.themes.push(d.theme);
    if (!group.indexes.includes(d.index)) group.indexes.push(d.index);
    group.count += 1;
    groups.set(key, group);
  }
  for (const g of groups.values()) g.indexes.sort((a, b) => a - b);
  return [...groups.values()];
}
