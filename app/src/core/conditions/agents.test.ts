// A project that calls for agents (design-live-follow §14.7 #21): git or agent files at the root, projects only.
import { describe, expect, it } from "vitest";
import type { Context } from "../../api";
import { agentFilesOf, callsForAgents } from "./agents";
import { neutralContext } from "./context.svelte";

const context = (over: Record<string, unknown>): Context =>
  ({ ...neutralContext(), workspace: "project", ...over }) as Context;

describe("calls for agents", () => {
  it("a project with git, or with any agent file at its root", () => {
    expect(
      callsForAgents(context({ git: { status: "repo", state: "clean" }, agentFiles: [] })),
    ).toBe(true);
    for (const name of ["AGENTS.md", "CLAUDE.md", ".mcp.json", ".claude/"]) {
      expect(callsForAgents(context({ agentFiles: [name] })), name).toBe(true);
    }
  });

  it("not without both, not outside a project, not before the fact is known", () => {
    expect(callsForAgents(context({ agentFiles: [] }))).toBe(false);
    expect(callsForAgents(context({}))).toBe(false);
    for (const workspace of ["notes", "singleFile", "welcome"]) {
      expect(callsForAgents(context({ workspace, agentFiles: ["CLAUDE.md"] })), workspace).toBe(
        false,
      );
    }
    expect(callsForAgents(context({ git: { status: "error", reason: "x" } }))).toBe(false);
  });

  it("reads only names from the fact", () => {
    expect(agentFilesOf(context({ agentFiles: ["CLAUDE.md", 3, null] }))).toEqual(["CLAUDE.md"]);
    expect(agentFilesOf(context({ agentFiles: "CLAUDE.md" }))).toEqual([]);
  });
});
