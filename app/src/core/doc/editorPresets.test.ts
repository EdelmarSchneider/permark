import { describe, expect, it } from "vitest";
import { EDITOR_PRESETS, presetOf } from "./editorPresets";

describe("editor presets", () => {
  it("name the choice a command is", () => {
    expect(presetOf("")).toBe("none");
    expect(presetOf("   ")).toBe("none");
    expect(presetOf("code -g {path}:{line}:{col}")).toBe("vscode");
    expect(presetOf("  zed {path}:{line}:{col} ")).toBe("zed");
    expect(presetOf("emacsclient +{line} {path}")).toBe("custom");
  });

  it("every preset has a path and a distinct command", () => {
    const commands = EDITOR_PRESETS.map((p) => p.command);
    expect(new Set(commands).size).toBe(commands.length);
    for (const command of commands) expect(command).toContain("{path}");
  });
});
