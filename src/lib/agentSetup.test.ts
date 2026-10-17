import { describe, expect, it } from "vitest";
import { claudeCodeCommand, hookJson, mcpJson, programWord } from "./agentSetup";

const WINDOWS = "C:\\Users\\Ada Lovelace\\AppData\\Local\\PerMark\\permark.exe";

describe("programWord", () => {
  it("is permark when the program is not known", () => {
    expect(programWord(null)).toBe("permark");
    expect(programWord("")).toBe("permark");
  });

  it("quotes a path with spaces and leaves a plain one alone", () => {
    expect(programWord(WINDOWS)).toBe(`"${WINDOWS}"`);
    expect(programWord("/usr/local/bin/permark")).toBe("/usr/local/bin/permark");
    expect(programWord('C:\\odd "name"\\permark.exe')).toBe(
      '"C:\\odd \\"name\\"\\permark.exe"',
    );
    // backslashes alone are enough: Git Bash would eat them
    expect(programWord("C:\\PerMark\\permark.exe")).toBe('"C:\\PerMark\\permark.exe"');
    expect(programWord("C:/Program Files/PerMark/permark.exe")).toBe(
      '"C:/Program Files/PerMark/permark.exe"',
    );
  });

  it("single-quotes a Unix path a shell would otherwise read into, so $ and backticks stay what they are", () => {
    expect(programWord("/Applications/PerMark.app/Contents/MacOS/permark")).toBe(
      "/Applications/PerMark.app/Contents/MacOS/permark",
    );
    expect(programWord("/home/ada/My Apps/PerMark.AppImage")).toBe(
      "'/home/ada/My Apps/PerMark.AppImage'",
    );
    expect(programWord("/home/ada/$HOME/permark")).toBe("'/home/ada/$HOME/permark'");
    expect(programWord("/home/ada/`id`/permark")).toBe("'/home/ada/`id`/permark'");
    expect(programWord('/odd "name"/permark')).toBe("'/odd \"name\"/permark'");
    expect(programWord("/ada's apps/permark")).toBe("'/ada'\\''s apps/permark'");
  });
});

describe("claudeCodeCommand", () => {
  it("is the documented line when the program is not known", () => {
    expect(claudeCodeCommand(null)).toBe("claude mcp add permark -- permark mcp");
  });

  it("names the program by its path", () => {
    expect(claudeCodeCommand("/usr/local/bin/permark")).toBe(
      "claude mcp add permark -- /usr/local/bin/permark mcp",
    );
    expect(claudeCodeCommand(WINDOWS)).toBe(`claude mcp add permark -- "${WINDOWS}" mcp`);
  });
});

describe("mcpJson", () => {
  it("is the one line of configuration every MCP client takes", () => {
    expect(JSON.parse(mcpJson(null))).toEqual({
      mcpServers: { permark: { command: "permark", args: ["mcp"] } },
    });
  });

  it("keeps a Windows path as it is, escaped the way JSON needs", () => {
    const text = mcpJson(WINDOWS);
    expect(JSON.parse(text).mcpServers.permark.command).toBe(WINDOWS);
    expect(text).toContain("\\\\Users\\\\Ada Lovelace");
  });
});

describe("hookJson", () => {
  it("runs permark context before every prompt", () => {
    expect(JSON.parse(hookJson(null))).toEqual({
      hooks: {
        UserPromptSubmit: [{ hooks: [{ type: "command", command: "permark context" }] }],
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
    const alias = "permark";
    expect(claudeCodeCommand(alias)).toBe("claude mcp add permark -- permark mcp");
    expect(JSON.parse(mcpJson(alias))).toEqual({
      mcpServers: { permark: { command: "permark", args: ["mcp"] } },
    });
    const hook = JSON.parse(hookJson(alias)).hooks.UserPromptSubmit[0].hooks[0].command;
    expect(hook).toBe("permark context");
  });
});
