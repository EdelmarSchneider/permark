import { describe, expect, it } from "vitest";
import type { AgentRequest, AgentSession } from "../../api";
import { sessionRows } from "./sessions";

const session = (id: string, name: string, folder = ""): AgentSession => ({
  id,
  name,
  startedMs: 1,
  folder,
  lastCallMs: null,
});

const request = (sessionId: string, tool: string, atMs: number): AgentRequest => ({
  session: sessionId,
  tool,
  subject: null,
  atMs,
  outcome: "ok",
});

describe("the rows of connected agents", () => {
  it("numbers duplicate names by start order and names each one's latest request", () => {
    const rows = sessionRows(
      [
        session("aaaaaaaa", "Claude Code"),
        session("bbbbbbbb", "Codex", "docs"),
        session("cccccccc", "Claude Code"),
      ],
      [request("cccccccc", "map", 9), request("aaaaaaaa", "read", 5), request("cccccccc", "x", 2)],
    );
    expect(rows.map((r) => r.label)).toEqual(["Claude Code", "Codex", "Claude Code · 2"]);
    expect(rows.map((r) => r.last)).toEqual([
      { tool: "read", atMs: 5 },
      null,
      { tool: "map", atMs: 9 },
    ]);
    expect(rows[1]?.session.folder).toBe("docs");
  });

  it("keeps the time of a call whose record is gone", () => {
    const [row] = sessionRows([{ ...session("aaaaaaaa", "A"), lastCallMs: 7 }], []);
    expect(row?.last).toEqual({ tool: null, atMs: 7 });
  });
});
