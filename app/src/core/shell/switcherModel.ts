// The tab strip's rules, pure: which tab a key goes to, the workspace a view shows, how a chip looks (hue, name).

import type { TabSummary, WindowView } from "../../api";

/** Switching (next | previous) needs another tab to switch to. */
export function canSwitch(list: readonly TabSummary[]): boolean {
  return list.length >= 2;
}

/**
 * The tab `step` places after `active` (negative = before), wrapping around; `null` when there is nothing to go to
 * (fewer than two tabs). An active tab that is not in the list counts as before the first.
 */
export function neighbor(
  list: readonly TabSummary[],
  active: number | null,
  step: 1 | -1,
): TabSummary | null {
  if (!canSwitch(list)) return null;
  const at = list.findIndex((t) => t.tab === active);
  const to = at < 0 ? (step === 1 ? 0 : list.length - 1) : (at + step + list.length) % list.length;
  return list[to] ?? null;
}

/** Tab `n` (1-based) of the strip; `null` when there is none. */
export function nth(list: readonly TabSummary[], n: number): TabSummary | null {
  return list[n - 1] ?? null;
}

/** The last tab of the strip. */
export function last(list: readonly TabSummary[]): TabSummary | null {
  return list.at(-1) ?? null;
}

/** The workspace a view shows (what stays on screen while another opens), `null` when it shows none. */
export function shownId(view: WindowView): number | null {
  switch (view.state) {
    case "showing":
      return view.workspace.id;
    case "opening":
      return view.showing?.id ?? null;
    case "welcome":
    case "closing":
      return null;
    default:
      return view satisfies never;
  }
}

/** A tag palette color (tokens `--tag-<hue>`, both themes). */
export type Hue = "blue" | "teal" | "green" | "purple" | "orange" | "gray" | "yellow";

/** The hues a project can wear: none that already means something (red = problem, yellow = Notes, gray = file). */
const PROJECT_HUES: readonly Hue[] = ["blue", "teal", "green", "purple", "orange"];

/** A workspace's color, the same in every window and every session: from its path for a project. */
export function hueOf(workspace: Pick<TabSummary, "kind" | "path">): Hue {
  switch (workspace.kind) {
    case "notes":
      return "yellow";
    case "singleFile":
    case "welcome":
      return "gray";
    case "project": {
      // FNV-1a over the case-folded path with `/` separators: stable across runs and spellings of the separator
      let hash = 0x811c9dc5;
      for (const ch of workspace.path.replaceAll("\\", "/").toLowerCase()) {
        hash ^= ch.codePointAt(0) ?? 0;
        hash = Math.imul(hash, 0x01000193) >>> 0;
      }
      return PROJECT_HUES[hash % PROJECT_HUES.length] ?? "blue";
    }
    default:
      return workspace.kind satisfies never;
  }
}

/**
 * A name cut for truncation in the middle: `head` shrinks with an ellipsis, `tail` always shows. A file keeps its
 * extension and a few letters before it in the tail; anything else its last letters. Short names have no tail.
 */
export function splitName(name: string, isFile: boolean): { head: string; tail: string } {
  const chars = Array.from(name);
  if (chars.length <= 14) return { head: name, tail: "" };
  const dot = isFile ? name.lastIndexOf(".") : -1;
  const ext = dot > 0 && name.length - dot <= 8 ? Array.from(name.slice(dot)).length : 0;
  const keep = ext > 0 ? Math.min(ext + 4, chars.length - 6) : 6;
  return { head: chars.slice(0, -keep).join(""), tail: chars.slice(-keep).join("") };
}

/** Below this width per chip, names keep their start only (px). */
export const TIGHT_CHIP = 150;

/** Whether `count` chips share `room` px so tightly that a name cut in the middle would show little of its start. */
export function chipsTight(room: number, count: number): boolean {
  return count > 0 && room / count < TIGHT_CHIP;
}
