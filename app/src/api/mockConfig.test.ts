import { describe, expect, it } from "vitest";
import { createMockConfig } from "./mockConfig";
import { createMockEditing } from "./mockEdit";
import { mockView } from "./mockDocs";

const config = createMockConfig();
const editing = createMockEditing(() => []).api;

async function sheet(path: string, text: string) {
  const result = await config.configSheet(path, text);
  if (!result.ok) throw new Error("refused");
  return result.value;
}

describe("mock render of an unsaved text", () => {
  it("a fixture document's own text gets the real backend's render", async () => {
    const source = await editing.editSource("README.md");
    if (!source.ok) throw new Error("no source");
    expect(await sheet("README.md", source.value.text)).toEqual(
      mockView("README.md", [], true)?.body,
    );
  });

  it("any other Markdown text: escaped blocks with their lines", async () => {
    const body = await sheet("notes/new.md", "# Title\n\n<b>x</b> & y\nmore\n");
    expect(body.kind).toBe("markdown");
    if (body.kind !== "markdown") return;
    expect(body.blocks.map((b) => [b.lineStart, b.lineEnd])).toEqual([
      [1, 1],
      [3, 4],
    ]);
    expect(body.blocks[1]?.html).toContain("&lt;b&gt;x&lt;/b&gt; &amp; y");
    expect(body.frontMatter).toBeNull();
  });

  it("config files keep their sheets", async () => {
    expect((await sheet(".gitignore", "node_modules/\n")).kind).toBe("sheet");
    expect((await sheet("notes/todo.txt", "x")).kind).toBe("text");
  });
});
