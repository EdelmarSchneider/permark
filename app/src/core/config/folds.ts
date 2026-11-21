// Which cards of a sheet are folded. Uses the read-only tree's row budgets, counted in drawn sheet rows: a card
// header is one row, a key one row, a list one row per item, a table its rows up to the ones a long table draws at
// most. Cards sit side by side (no levels), so where the tree opens the levels that fit, a sheet opens its leading
// cards that fit and folds the rest.
import type { Group, Row, Sheet } from "../../api";
import { EXPAND_BUDGET, OPEN_BUDGET } from "../doc/tree";
import { VIRTUAL_FROM } from "./tableWindow";

export const cardKey = (group: Group): string => `${group.path}@${group.line}`;

/** Whether the cards show a header (and so can fold): one untitled card has none. */
export const isHeaded = (sheet: Sheet): boolean =>
  sheet.groups.length > 1 || sheet.groups.some((g) => g.title !== "");

function rowsOf(row: Row): number {
  const value = row.value;
  if (value.type === "list") return Math.max(1, value.items.length);
  if (value.type === "table") return 1 + Math.min(value.rows.length, VIRTUAL_FROM);
  return 1;
}

const cardRows = (group: Group): number => group.rows.reduce((sum, row) => sum + rowsOf(row), 0);

/** The card whose lines hold `line` (the last such card: a nested card follows its parent); `null`: none. */
export function cardAt(sheet: Sheet, line: number): string | null {
  let found: Group | null = null;
  for (const group of sheet.groups) if (group.line <= line && line <= group.end) found = group;
  return found ? cardKey(found) : null;
}

/** Rows drawn with every card open. */
export function drawnRows(sheet: Sheet): number {
  return sheet.groups.reduce((sum, group) => sum + 1 + cardRows(group), 0);
}

/**
 * The folded cards: `null` = how a document opens (within `OPEN_BUDGET`), `true` = "Expand all" (within
 * `EXPAND_BUDGET`), `false` = "Collapse all" (every card, headers stay). A sheet without headers never folds.
 */
export function foldsOf(sheet: Sheet, open: boolean | null): ReadonlySet<string> {
  if (!isHeaded(sheet)) return new Set();
  if (open === false) return new Set(sheet.groups.map(cardKey));
  const budget = open === null ? OPEN_BUDGET : EXPAND_BUDGET;
  const folded = new Set<string>();
  let rows = sheet.groups.length;
  for (const group of sheet.groups) {
    if (folded.size === 0) rows += cardRows(group);
    if (folded.size > 0 || rows > budget) folded.add(cardKey(group));
  }
  return folded;
}
