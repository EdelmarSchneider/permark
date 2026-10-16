import { describe, expect, it } from "vitest";
import { fileIconOf } from "./fileIcons";

describe("fileIconOf", () => {
  it.each([
    ["README.md", "file-text"],
    ["docs/guide.markdown", "file-text"],
    ["assets/logo.png", "image"],
    ["assets/Logo.SVG", "image"],
    ["sounds/ping.wav", "music"],
    ["talk.opus", "music"],
    ["clips/demo.mp4", "video"],
    ["clips/demo.MKV", "video"],
    ["src/main.rs", "file"],
    ["data.json", "file"],
    ["Makefile", "file"],
  ])("%s gets %s", (path, icon) => {
    expect(fileIconOf(path)).toBe(icon);
  });
});
