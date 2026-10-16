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
    expect(programWord('C:\\odd "name"\\trailmark.exe')).toBe(
      '"C:\\odd \\"name\\"\\trailmark.exe"',
    );
    // backslashes alone are enough: Git Bash would eat them
    expect(programWord("C:\\TrailMark\\trailmark.exe")).toBe('"C:\\TrailMark\\trailmark.exe"');
    expect(programWord("C:/Program Files/TrailMark/trailmark.exe")).toBe(
      '"C:/Program Files/TrailMark/trailmark.exe"',
    );
  });

  it("single-quotes a Unix path a shell would otherwise read into, so $ and backticks stay what they are", () => {
    expect(programWord("/Applications/TrailMark.app/Contents/MacOS/trailmark")).toBe(
      "/Applications/TrailMark.app/Contents/MacOS/trailmark",
    );
    expect(programWord("/home/ada/My Apps/TrailMark.AppImage")).toBe(
      "'/home/ada/My Apps/TrailMark.AppImage'",
    );
    expect(programWord("/home/ada/$HOME/trailmark")).toBe("'/home/ada/$HOME/trailmark'");
    expect(programWord("/home/ada/`id`/trailmark")).toBe("'/home/ada/`id`/trailmark'");
    expect(programWord('/odd "name"/trailmark')).toBe("'/odd \"name\"/trailmark'");
    expect(programWord("/ada's apps/trailmark")).toBe("'/ada'\\''s apps/trailmark'");
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

// The Microsoft Store build tells the window that its program is the alias its package puts on the PATH (the path inside
// the package is not meant to be run), so the lines come out as the documented ones, with nothing quoted.
describe("the lines of the Microsoft Store build", () => {
  it("name the alias", () => {
    const alias = "trailmark";
    expect(claudeCodeCommand(alias)).toBe("claude mcp add trailmark -- trailmark mcp");
    expect(JSON.parse(mcpJson(alias))).toEqual({
      mcpServers: { trailmark: { command: "trailmark", args: ["mcp"] } },
    });
    const hook = JSON.parse(hookJson(alias)).hooks.UserPromptSubmit[0].hooks[0].command;
    expect(hook).toBe("trailmark context");
  });
});
