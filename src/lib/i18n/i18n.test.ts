import { describe, expect, it } from "vitest";
import { t } from "./index";

describe("t", () => {
  it("returns the English string for a key", () => {
    expect(t("app.name")).toBe("TrailMark");
  });
});
