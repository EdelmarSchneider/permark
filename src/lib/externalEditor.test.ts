import { describe, expect, it } from "vitest";
import {
  EDITOR_CHOICES,
  EDITOR_PRESETS,
  commandOf,
  presetOf,
  previewCommand,
  sampleFile,
  usesSystemApp,
} from "./externalEditor";

describe("presets", () => {
  it("offers the system default first and a free text last", () => {
    expect(EDITOR_CHOICES).toEqual([
      "system",
      "vscode",
      "cursor",
      "zed",
      "sublime",
      "jetbrains",
      "custom",
    ]);
  });

  it("has the commands of the design", () => {
    const commands = Object.fromEntries(EDITOR_PRESETS.map((p) => [p.id, p.command]));
    expect(commands).toEqual({
      system: "",
      vscode: "code -g {path}:{line}:{col}",
      cursor: "cursor -g {path}:{line}:{col}",
      zed: "zed {path}:{line}:{col}",
      sublime: "subl {path}:{line}:{col}",
      jetbrains: "idea --line {line} {path}",
    });
  });

  it("maps a saved command back to its preset", () => {
    for (const preset of EDITOR_PRESETS) {
      expect(presetOf(preset.command)).toBe(preset.id);
      expect(presetOf(`  ${preset.command}\n`)).toBe(preset.id);
    }
    expect(presetOf("")).toBe("system");
    expect(presetOf("   ")).toBe("system");
  });

  it("calls anything else custom", () => {
    expect(presetOf("code {path}")).toBe("custom");
    expect(presetOf("code -g  {path}:{line}:{col}")).toBe("custom");
    expect(presetOf("vim +{line} {path}")).toBe("custom");
  });

  it("fills in a preset's command, and none for custom", () => {
    expect(commandOf("vscode")).toBe("code -g {path}:{line}:{col}");
    expect(commandOf("system")).toBe("");
    expect(commandOf("custom")).toBeNull();
  });
});

describe("previewCommand", () => {
  it("fills in the placeholders", () => {
    expect(previewCommand("code -g {path}:{line}:{col}", "/p/src/main.rs", 42, 7)).toBe(
      "code -g /p/src/main.rs:42:7",
    );
    expect(previewCommand("idea --line {line} {path}", "/p/a.rs", 3, 1)).toBe(
      "idea --line 3 /p/a.rs",
    );
  });

  it("quotes a path with spaces, as one argument", () => {
    expect(previewCommand("zed {path}:{line}", "C:\\My Docs\\a.rs", 5, 1)).toBe(
      'zed "C:\\My Docs\\a.rs":5',
    );
  });

  it("fills every occurrence and leaves other text alone", () => {
    expect(previewCommand("ed {line} {line} {foo}", "/a", 2, 1)).toBe("ed 2 2 {foo}");
  });

  it("is empty for no command", () => {
    expect(previewCommand("  ", "/a", 1, 1)).toBe("");
  });
});

describe("sampleFile", () => {
  it("joins the folder and the file with the folder's own separator", () => {
    expect(sampleFile("F:\\proj", "docs/a.md")).toBe("F:\\proj\\docs\\a.md");
    expect(sampleFile("/home/me/proj/", "docs/a.md")).toBe("/home/me/proj/docs/a.md");
    expect(sampleFile("F:\\proj\\", "a.md")).toBe("F:\\proj\\a.md");
  });

  it("is the relative path when there is no folder", () => {
    expect(sampleFile(null, "docs/a.md")).toBe("docs/a.md");
    expect(sampleFile("", "docs/a.md")).toBe("docs/a.md");
  });
});

describe("usesSystemApp", () => {
  it("sends images, sound, video and PDF files to the system's application", () => {
    for (const path of [
      "a.png",
      "img/b.JPG",
      "c.svg",
      "d.pdf",
      "e.PDF",
      "docs/f.webp",
      "g.mp4",
      "h.MP3",
      "clips/i.mkv",
    ]) {
      expect(usesSystemApp(path), path).toBe(true);
    }
  });

  it("keeps text and code for the editor", () => {
    for (const path of ["a.md", "src/main.rs", "b.json", "Makefile", "c.txt", "d.ts"]) {
      expect(usesSystemApp(path), path).toBe(false);
    }
  });
});
