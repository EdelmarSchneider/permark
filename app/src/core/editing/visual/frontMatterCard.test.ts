// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { buildDoc, compose } from "./compose";
import { createCore, type Core } from "./core";

let core: Core;
let replace: ((source: string) => void) | null = null;
let drawn = 0;

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({
    host,
    locked: {
      reasonText: (reason) => reason,
      editLabel: "Source",
      cardText: "Properties",
      edit: () => {},
      // the properties card takes the front matter's place and edits through `replace`
      draw: (_body, kind, _source, put) => {
        if (kind !== "frontMatter") return null;
        replace = put;
        drawn += 1;
        return () => {};
      },
    },
  });
});

afterAll(async () => {
  await core.destroy();
});

const TEXT = "---\ntitle: A plan\nowner: ana\n---\n\nBody words here.\n";
const SEGMENTS = [
  { lineStart: 1, lineEnd: 4, kind: "frontMatter", lock: "frontMatter" },
  { lineStart: 6, lineEnd: 6, kind: "paragraph", lock: null },
] as const;

describe("the front matter as editable properties", () => {
  it("replaces the block's source: the document gets those bytes and nothing else changes", async () => {
    const built = await buildDoc(core, TEXT, SEGMENTS);
    core.setDoc(built.doc);
    expect(drawn).toBeGreaterThan(0);
    const block = document.querySelector(".pm-locked--card");
    expect(block).not.toBeNull();
    expect(block?.querySelector(".pm-locked__why")?.textContent).toBe("Properties");
    // untouched: the same text, no block changed
    expect(compose(core, built, core.view.state.doc).text).toBe(TEXT);
    replace?.("---\ntitle: A better plan\nowner: ana\n---");
    const composed = compose(core, built, core.view.state.doc);
    expect(composed.text).toBe(TEXT.replace("A plan", "A better plan"));
    expect(composed.changed).toBe(1);
  });

  it("a front matter block with no card keeps its source and the reason", async () => {
    const plain = await createCore({
      host: document.body.appendChild(document.createElement("div")),
      locked: {
        reasonText: (reason) => `locked: ${reason}`,
        editLabel: "Source",
        edit: () => {},
        draw: () => null,
      },
    });
    const built = await buildDoc(plain, TEXT, SEGMENTS);
    plain.setDoc(built.doc);
    const blocks = document.querySelectorAll(".pm-locked");
    const last = blocks[blocks.length - 1];
    expect(last?.classList.contains("pm-locked--card")).toBe(false);
    expect(last?.querySelector(".pm-locked__source")?.textContent).toBe(
      "---\ntitle: A plan\nowner: ana\n---",
    );
    expect(last?.querySelector(".pm-locked__why")?.textContent).toBe("locked: frontMatter");
    await plain.destroy();
  });
});
