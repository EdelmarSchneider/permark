import { describe, expect, it } from "vitest";
import { t, tn } from "./index";

describe("t", () => {
  it("returns the English string for a key", () => {
    expect(t("app.name")).toBe("TrailMark");
  });

  it("fills placeholders", () => {
    expect(t("welcome.openFailed", { path: "F:/x", message: "access denied" })).toBe(
      "Can't open F:/x: access denied",
    );
  });

  it("leaves unknown placeholders visible", () => {
    expect(t("doc.saved", {})).toBe("saved {time} ago");
  });
});

describe("tn", () => {
  it("picks the singular and plural forms", () => {
    expect(tn("feed.saves", 1)).toBe("1 save");
    expect(tn("feed.saves", 3)).toBe("3 saves");
    expect(tn("feed.saves", 0)).toBe("0 saves");
  });

  it("formats large counts and extra parameters", () => {
    expect(tn("toast.docsChanged", 1200)).toBe("1,200 docs changed");
    expect(tn("doc.removedBlocks", 1, { time: "10:42" })).toBe("1 block removed · 10:42");
  });
});
