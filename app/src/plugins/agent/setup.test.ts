import { describe, expect, it } from "vitest";
import { claudeCodeCommand, hookJson, mcpJson, programWord } from "./setup";

describe("agent setup lines", () => {
  it("name the program so a shell takes it as one word", () => {
    expect(programWord(null)).toBe("permark");
    expect(programWord("C:\\Tools\\PerMark.exe")).toBe('"C:\\Tools\\PerMark.exe"');
    expect(programWord("C:/Tools/PerMark.exe")).toBe("C:/Tools/PerMark.exe");
    expect(programWord("C:\\Program Files\\PerMark\\PerMark.exe")).toBe(
      '"C:\\Program Files\\PerMark\\PerMark.exe"',
    );
    expect(programWord("/usr/bin/permark")).toBe("/usr/bin/permark");
    expect(programWord("/home/a b/Perm$ark's.AppImage")).toBe(
      "'/home/a b/Perm$ark'\\''s.AppImage'",
    );
  });

  it("keep the command line agents already use: <program> mcp", () => {
    expect(claudeCodeCommand("/opt/permark")).toBe("claude mcp add permark -- /opt/permark mcp");
    const json = JSON.parse(mcpJson("C:\\P\\PerMark.exe")) as {
      mcpServers: { permark: { command: string; args: string[] } };
    };
    expect(json.mcpServers.permark).toEqual({ command: "C:\\P\\PerMark.exe", args: ["mcp"] });
    const hook = JSON.parse(hookJson(null)) as {
      hooks: { UserPromptSubmit: { hooks: { command: string }[] }[] };
    };
    expect(hook.hooks.UserPromptSubmit[0]?.hooks[0]?.command).toBe("permark context");
  });
});
