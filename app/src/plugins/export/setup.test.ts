import { describe, expect, it } from "vitest";
import type { SetupView } from "../../api/generated/SetupView";
import { offeredSetup, regionPaper } from "./setup";

const view = (stored: boolean): SetupView => ({
  setup: { size: "a3", orientation: "landscape", margins: "wide", pageNumbers: false },
  page: { widthMm: 420, heightMm: 297, marginMm: 25 },
  stored,
});

describe("region paper", () => {
  it("Letter where a Letter region is written in the tag, A4 elsewhere", () => {
    expect(regionPaper("en-US")).toBe("letter");
    expect(regionPaper("es-MX")).toBe("letter");
    expect(regionPaper("fr-CA")).toBe("letter");
    expect(regionPaper("pt-BR")).toBe("a4");
    expect(regionPaper("en-GB")).toBe("a4");
  });

  it("a bare language or a broken tag keeps A4", () => {
    expect(regionPaper("en")).toBe("a4");
    expect(regionPaper("es")).toBe("a4");
    expect(regionPaper("not a tag!")).toBe("a4");
  });

  it("the region's paper applies only while nothing was chosen", () => {
    expect(offeredSetup(view(false), "en-US")).toEqual({ ...view(false).setup, size: "letter" });
    expect(offeredSetup(view(true), "en-US")).toEqual(view(true).setup);
  });
});
