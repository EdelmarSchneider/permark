import { describe, expect, it } from "vitest";
import type { WindowView } from "../api";
import { windowTitle } from "./title";

describe("window title", () => {
  const view: WindowView = {
    state: "showing",
    workspace: { kind: "project", id: 1, name: "docs", root: "C:/docs", stateLocation: "profile" },
    document: "notes/plan.md",
  };

  it("names the document and the project, with a star while edits are unsaved", () => {
    expect(windowTitle(view)).toBe("plan.md — docs — PerMark");
    expect(windowTitle(view, true)).toBe("*plan.md — docs — PerMark");
  });
});
