import { describe, expect, it } from "vitest";
import { errorKind, errorText } from "./errors";

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

describe("errorKind", () => {
  it("reads the stable kind of a serialized error", () => {
    expect(errorKind({ kind: "exists", message: "a file named a.md already exists" })).toBe(
      "exists",
    );
  });

  it("is null for anything without a kind", () => {
    expect(errorKind("boom")).toBeNull();
    expect(errorKind(new Error("x"))).toBeNull();
    expect(errorKind({ kind: 3 })).toBeNull();
    expect(errorKind(null)).toBeNull();
  });
});
