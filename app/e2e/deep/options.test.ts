import { describe, expect, it } from "vitest";
import { LAYERS, parseOptions, UsageError } from "./options.ts";

describe("parseOptions", () => {
  it("runs everything by default", () => {
    expect(parseOptions([])).toEqual({
      only: LAYERS,
      update: false,
      accept: null,
      quick: false,
      grep: null,
    });
  });

  it("reads each option", () => {
    expect(parseOptions(["--only", "render", "--update", "--quick", "--grep", "math"])).toEqual({
      only: ["render"],
      update: true,
      accept: null,
      quick: true,
      grep: "math",
    });
    expect(parseOptions(["--accept", "demo__README/*"]).accept).toBe("demo__README/*");
  });

  it("refuses unknown options, missing values and update with accept", () => {
    for (const args of [
      ["--only", "pixels"],
      ["--only"],
      ["--accept", "--quick"],
      ["--fast"],
      ["--accept", "all", "--update"],
    ]) {
      expect(() => parseOptions(args), args.join(" ")).toThrow(UsageError);
    }
  });
});
