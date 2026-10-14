import { describe, expect, it } from "vitest";
import { errorText } from "./errors";

describe("errorText", () => {
  it("reads strings and Error instances", () => {
    expect(errorText("boom")).toBe("boom");
    expect(errorText(new Error("bad path"))).toBe("bad path");
  });

  it("reads a message field from a serialized error", () => {
    expect(errorText({ kind: "io", message: "access denied" })).toBe("access denied");
  });

  it("unwraps externally tagged enums", () => {
    expect(errorText({ notFound: "docs/a.md" })).toBe("notFound: docs/a.md");
    expect(errorText({ io: { message: "sharing violation" } })).toBe("sharing violation");
  });

  it("falls back to JSON for unknown shapes", () => {
    expect(errorText({ a: 1, b: 2 })).toBe('{"a":1,"b":2}');
    expect(errorText(42)).toBe("42");
  });
});
