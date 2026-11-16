import { describe, expect, it } from "vitest";
import type { Sheet } from "../../api";
import { EXPAND_BUDGET, OPEN_BUDGET } from "../doc/tree";
import { bigSheet, scalar, scalarRow } from "../testing/sheets";
import { drawnRows, foldsOf } from "./folds";

const keyOf = (sheet: Sheet, index: number): string => {
  const group = sheet.groups[index];
  return group ? `${group.path}@${group.line}` : "";
};

describe("folds of a sheet", () => {
  it("count a header per card and a row per key", () => {
    expect(drawnRows(bigSheet(3, 10))).toBe(33);
  });

  it("count a list by its items", () => {
    const sheet = bigSheet(1, 0);
    const items = Array.from({ length: 50 }, (_, i) => scalar(`l[${i}]`));
    sheet.groups[0]?.rows.push({ ...scalarRow("l"), value: { type: "list", items } });
    expect(drawnRows(sheet)).toBe(1 + 50);
  });

  it("open everything that fits, nothing folded", () => {
    expect(foldsOf(bigSheet(4, 10), null).size).toBe(0);
    expect(foldsOf(bigSheet(4, 10), true).size).toBe(0);
  });

  it("open the leading cards that fit the budget, fold the rest", () => {
    const sheet = bigSheet(6, Math.ceil(OPEN_BUDGET / 4));
    // 6 headers + 3 open cards fit; the 4th would pass the budget
    expect([...foldsOf(sheet, null)]).toEqual([keyOf(sheet, 3), keyOf(sheet, 4), keyOf(sheet, 5)]);
    // Expand all has the larger budget
    expect(foldsOf(sheet, true).size).toBe(0);
    expect(foldsOf(bigSheet(3, EXPAND_BUDGET), true).size).toBe(3);
  });

  it("fold every card on Collapse all", () => {
    expect(foldsOf(bigSheet(3, 2), false).size).toBe(3);
  });

  it("never fold the only card when it has no header to open it again", () => {
    const [group] = bigSheet(1, OPEN_BUDGET * 2).groups;
    if (!group) throw new Error("no card");
    const sheet: Sheet = { kind: "config", groups: [{ ...group, path: "", title: "" }] };
    expect(foldsOf(sheet, null).size).toBe(0);
    expect(foldsOf(sheet, false).size).toBe(0);
  });
});
