import { describe, expect, it } from "vitest";
import { claudeCodeCommand, hookJson, mcpJson, programWord } from "./agentSetup";

const WINDOWS = "C:\\Users\\Ada Lovelace\\AppData\\Local\\TrailMark\\trailmark.exe";

describe("programWord", () => {
  it("is trailmark when the program is not known", () => {
    expect(programWord(null)).toBe("trailmark");
    expect(programWord("")).toBe("trailmark");
  });

  it("quotes a path with spaces and leaves a plain one alone", () => {
    expect(programWord(WINDOWS)).toBe(`"${WINDOWS}"`);
    expect(programWord("/usr/local/bin/trailmark")).toBe("/usr/local/bin/trailmark");
    expect(programWord('/odd "name"/trailmark')).toBe('"/odd \\"name\\"/trailmark"');
    // backslashes alone are enough: Git Bash would eat them
    expect(programWord("C:\\TrailMark\\trailmark.exe")).toBe('"C:\\TrailMark\\trailmark.exe"');
  });
});

describe("claudeCodeCommand", () => {
  it("is the documented line when the program is not known", () => {
    expect(claudeCodeCommand(null)).toBe("claude mcp add trailmark -- trailmark mcp");
  });

  it("names the program by its path", () => {
    expect(claudeCodeCommand("/usr/local/bin/trailmark")).toBe(
      "claude mcp add trailmark -- /usr/local/bin/trailmark mcp",
    );
    expect(claudeCodeCommand(WINDOWS)).toBe(`claude mcp add trailmark -- "${WINDOWS}" mcp`);
  });
});

describe("mcpJson", () => {
  it("is the one line of configuration every MCP client takes", () => {
    expect(JSON.parse(mcpJson(null))).toEqual({
      mcpServers: { trailmark: { command: "trailmark", args: ["mcp"] } },
    });
  });

  it("keeps a Windows path as it is, escaped the way JSON needs", () => {
    const text = mcpJson(WINDOWS);
    expect(JSON.parse(text).mcpServers.trailmark.command).toBe(WINDOWS);
    expect(text).toContain("\\\\Users\\\\Ada Lovelace");
  });
});

describe("hookJson", () => {
  it("runs trailmark context before every prompt", () => {
    expect(JSON.parse(hookJson(null))).toEqual({
      hooks: {
        UserPromptSubmit: [{ hooks: [{ type: "command", command: "trailmark context" }] }],
      },
    });
  });

  it("quotes the program inside the command", () => {
    const command = JSON.parse(hookJson(WINDOWS)).hooks.UserPromptSubmit[0].hooks[0].command;
    expect(command).toBe(`"${WINDOWS}" context`);
  });
});
