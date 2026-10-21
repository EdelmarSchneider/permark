// Pure: from two snapshots (before | after an action) + what the page recorded, the step's facts and flags
// (spec: ux-probe.md §7). No browser here; tested by facts.test.ts.

import type { FocusFact, Observed, Snapshot } from "./page.ts";

export type Effect = "none" | "selfOnly" | "elsewhere";

export type Change = {
  mutations: number;
  /** Where the page changed (elements, max 10). */
  changed: string[];
  firstAtMs: number | null;
  outside: number;
  focusMoved: boolean;
  effect: Effect;
  explained: boolean;
  textAdded: string[];
  textRemoved: string[];
};

export type Thumb = { selector: string; visible: boolean; width: number };

export type Flag = { id: string; detail: string };

export type ActionKind = "click" | "press" | "other" | "none";

const MAX_LINES = 40;

/** Lines in `after` not in `before` and the reverse, as multisets (order-insensitive), capped. */
export function textDiff(
  before: readonly string[],
  after: readonly string[],
): { added: string[]; removed: string[] } {
  const count = new Map<string, number>();
  for (const l of before) count.set(l, (count.get(l) ?? 0) + 1);
  const added: string[] = [];
  for (const l of after) {
    const n = count.get(l) ?? 0;
    if (n > 0) count.set(l, n - 1);
    else added.push(l);
  }
  const removed: string[] = [];
  for (const [l, n] of count) for (let i = 0; i < n; i++) removed.push(l);
  return { added: added.slice(0, MAX_LINES), removed: removed.slice(0, MAX_LINES) };
}

/** What the action did: nothing, only the control (+ its group) itself, or something elsewhere. */
export function effectOf(observed: Observed): Effect {
  if (observed.mutations === 0 && !observed.focusMoved) return "none";
  if (observed.outside === 0 && !observed.focusMoved) return "selfOnly";
  return "elsewhere";
}

/** New text in alerts, status + live regions, tooltips, dialogs: the page said something. */
export function explained(before: readonly string[], after: readonly string[]): boolean {
  return textDiff(before, after).added.length > 0;
}

export function changeOf(before: Snapshot, after: Snapshot, observed: Observed): Change {
  const diff = textDiff(before.text, after.text);
  return {
    mutations: observed.mutations,
    changed: observed.changed,
    firstAtMs: observed.firstAtMs,
    outside: observed.outside,
    focusMoved: observed.focusMoved,
    effect: effectOf(observed),
    explained: explained(before.live, after.live),
    textAdded: diff.added,
    textRemoved: diff.removed,
  };
}

/** Names a key combo goes by in hints: "Escape" -> Esc | Escape; "Control+Shift+k" -> Ctrl+Shift+K. */
export function keyNames(combo: string): string[] {
  const parts = combo.split("+").map((p) => {
    if (p === "Control") return "Ctrl";
    if (p === "Meta") return "Cmd";
    if (p.length === 1) return p.toUpperCase();
    return p;
  });
  const last = parts[parts.length - 1] ?? "";
  const names = new Set<string>([parts.join("+")]);
  if (last === "Escape") names.add([...parts.slice(0, -1), "Esc"].join("+"));
  return [...names].map((n) => n.toLowerCase());
}

/** Visible hints that name the pressed key. */
export function promisedBy(
  combo: string,
  hints: Snapshot["keyHints"],
): { element: string; hint: string }[] {
  const names = keyNames(combo);
  return hints.filter((h) =>
    h.hint
      .split(/[\s,|]+/)
      .map((t) => t.trim().toLowerCase())
      .some((t) => names.includes(t)),
  );
}

function focusFlags(before: FocusFact, after: FocusFact, observed: Observed): Flag[] {
  const flags: Flag[] = [];
  if (after.kind === "body") {
    if (before.kind === "element" && observed.previousFocusAlive && observed.focusMoved) {
      flags.push({ id: "focus-lost", detail: `focus left ${before.selector} for the page body` });
    }
    return flags;
  }
  if (after.visible && after.covered >= 1) {
    flags.push({
      id: "focus-obscured",
      detail: `${after.selector} hidden under ${after.coveredBy ?? "?"}`,
    });
  } else if (after.visible && after.covered > 0) {
    flags.push({
      id: "focus-partly-obscured",
      detail: `${after.selector} ${Math.round(after.covered * 100)}% under ${after.coveredBy ?? "?"}`,
    });
  }
  if (after.clipped || !after.inViewport) {
    flags.push({
      id: "focus-clipped",
      detail: `${after.selector} cut by a scroll area or off screen`,
    });
  }
  if (after.focusVisible && !after.indicator) {
    flags.push({
      id: "focus-no-indicator",
      detail: `${after.selector}: :focus-visible, no outline | shadow`,
    });
  }
  if (
    after.current === false &&
    after.group !== null &&
    before.kind === "element" &&
    before.group !== null &&
    before.group.selector === after.group.selector &&
    before.group.current !== after.group.current
  ) {
    flags.push({
      id: "focus-stale-in-group",
      detail: `current moved "${before.group.current ?? "?"}" -> "${after.group.current ?? "?"}"; focus stays on "${after.name}"${after.focusVisible ? " (ring shown)" : ""}`,
    });
  }
  return flags;
}

/** Every flag of one probed step (spec: ux-probe.md §7 Flags). */
export function flagsOf(input: {
  action: ActionKind;
  key?: string;
  before: Snapshot | null;
  after: Snapshot;
  change: Change | null;
  observed: Observed | null;
  thumbs: readonly Thumb[];
  axe: readonly { id: string }[];
  expectChange: boolean;
  /** The clicked option | tab | toggle was already the chosen one. */
  selected?: boolean;
}): Flag[] {
  const { action, before, after, change, observed } = input;
  const flags: Flag[] = [];
  if (before !== null && observed !== null)
    flags.push(...focusFlags(before.focus, after.focus, observed));
  else flags.push(...focusFlags({ kind: "body" }, after.focus, { ...NO_OBSERVATION }));
  for (const p of after.paintsOver) {
    flags.push({
      id: "paints-over-region",
      detail: `${p.element} (in ${p.from}) drawn over ${p.over}`,
    });
  }
  const areas = after.regions.filter((r) => !r.overlay);
  for (let i = 0; i < areas.length; i++) {
    for (let j = i + 1; j < areas.length; j++) {
      const a = areas[i];
      const b = areas[j];
      if (!a || !b) continue;
      const w = Math.min(a.box.x + a.box.w, b.box.x + b.box.w) - Math.max(a.box.x, b.box.x);
      const h = Math.min(a.box.y + a.box.h, b.box.y + b.box.h) - Math.max(a.box.y, b.box.y);
      if (w > 1 && h > 1)
        flags.push({ id: "region-overlap", detail: `${a.selector} x ${b.selector}: ${w}x${h}px` });
    }
  }
  for (const c of after.covered) {
    flags.push({ id: "control-covered", detail: `"${c.name}" ${c.element} under ${c.by}` });
  }
  if (change !== null && !change.explained && input.selected !== true) {
    if (action === "click" && change.effect === "none") {
      flags.push({ id: "dead-control", detail: "click changed nothing on screen, no message" });
    }
    if (action === "click" && change.effect === "selfOnly") {
      flags.push({
        id: "no-effect-beyond-control",
        detail: "only the control (or its option group) changed; nothing else followed, no message",
      });
    }
    if (action === "press" && change.effect === "none") {
      flags.push({
        id: "dead-key",
        detail: `${input.key ?? "key"} changed nothing on screen, no message`,
      });
      const promised = promisedBy(input.key ?? "", after.keyHints);
      if (promised.length > 0) {
        flags.push({
          id: "dead-key-promised",
          detail: `promised by ${promised.map((p) => `${p.element} "(${p.hint})"`).join(", ")}`,
        });
      }
    }
  }
  if (
    input.expectChange &&
    change !== null &&
    change.textAdded.length === 0 &&
    change.textRemoved.length === 0
  ) {
    flags.push({ id: "no-change", detail: "watched region shows the same text after the action" });
  }
  for (const t of input.thumbs) {
    if (!t.visible)
      flags.push({
        id: "scrollbar-invisible",
        detail: `${t.selector}: content scrolls, no thumb drawn`,
      });
    else if (t.width < 6)
      flags.push({ id: "scrollbar-thin", detail: `${t.selector}: thumb ${t.width}px wide` });
  }
  for (const v of input.axe) flags.push({ id: `axe:${v.id}`, detail: "axe violation" });
  return flags;
}

const NO_OBSERVATION: Observed = {
  mutations: 0,
  firstAtMs: null,
  outside: 0,
  lastAtMs: null,
  focusMoved: false,
  previousFocusAlive: true,
  changed: [],
};

// ---- calibration expectations ----

export type Expectation = { step: string; flag: string; absent?: boolean };
export type StepFlags = { name: string; flags: Flag[] };

/** Expectations not met by the steps' flags (empty = pass). A flag id ending in `*` matches by prefix. */
export function unmet(expect: readonly Expectation[], steps: readonly StepFlags[]): string[] {
  const misses: string[] = [];
  for (const e of expect) {
    const step = steps.find((s) => s.name === e.step);
    if (!step) {
      misses.push(`step "${e.step}" not run`);
      continue;
    }
    const hit = step.flags.some((f) =>
      e.flag.endsWith("*") ? f.id.startsWith(e.flag.slice(0, -1)) : f.id === e.flag,
    );
    if (e.absent === true && hit) misses.push(`${e.step}: unexpected ${e.flag}`);
    if (e.absent !== true && !hit) misses.push(`${e.step}: missing ${e.flag}`);
  }
  return misses;
}
