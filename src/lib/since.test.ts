import { describe, expect, it } from "vitest";
import { groupSince, isDubiousOwnership, safeDirectoryCommand } from "./since";

describe("groupSince", () => {
  const groups = groupSince([
    { path: "docs/specs/a.md", state: "modified", modifiedMs: 100 },
    { path: "plan.md", state: "modified", modifiedMs: 500 },
    { path: "docs/specs/b.md", state: "added", modifiedMs: 300 },
    { path: "docs/research/c.md", state: "untracked", modifiedMs: 200 },
  ]);

  it("groups by folder with the newest group first", () => {
    expect(groups.map((g) => g.folder)).toEqual(["", "docs/specs", "docs/research"]);
  });

  it("sorts files inside a group newest first and adds the file name", () => {
    expect(groups[1].files.map((f) => f.name)).toEqual(["b.md", "a.md"]);
  });

  it("returns nothing for no files", () => {
    expect(groupSince([])).toEqual([]);
  });
});

describe("git error helpers", () => {
  it("recognizes the dubious ownership error", () => {
    expect(isDubiousOwnership("detected dubious ownership in repository at 'F:/x'")).toBe(true);
    expect(isDubiousOwnership("repository not found")).toBe(false);
  });

  it("builds the safe.directory command with forward slashes", () => {
    expect(safeDirectoryCommand("F:\\work\\big-repo")).toBe(
      "git config --global --add safe.directory F:/work/big-repo",
    );
  });
});
