import { describe, expect, it } from "vitest";
import { LOCALES } from "../i18n/locales";
import { loadTable } from "../i18n/table";
import { reasonKey, refusalKey } from "./reasons";

describe("config sheet reasons", () => {
  it("a value with escape codes the sheet can't keep says so, in every language", async () => {
    expect(reasonKey("escape")).toBe("config.reason.escape");
    expect(refusalKey("escape")).toBe("config.reason.escape");
    for (const locale of LOCALES) {
      const loaded = await loadTable(locale);
      const table: Record<string, string> = loaded.status === "loaded" ? loaded.table : {};
      expect(table["config.reason.escape"], locale).toMatch(/\S/);
    }
  });

  it("a word nobody knows falls back to the general reason", () => {
    expect(reasonKey("somethingNew")).toBe("config.reason.other");
    expect(refusalKey("somethingNew")).toBe("config.refused.other");
  });
});
