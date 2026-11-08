// @vitest-environment jsdom
import "./jsdomShims";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { TextSelection } from "@milkdown/kit/prose/state";
import type { FrontMatter } from "../../../api/generated/FrontMatter";
import { buildDoc, compose, type Built } from "./compose";
import { createCore, type Core, type LockedHost } from "./core";
import type { LockedRender } from "./lockedRender";

let core: Core;
let replace: ((source: string) => void) | null = null;
let looks = new WeakMap<PMNode, LockedRender>();

const FRONT: FrontMatter = {
  syntax: "yaml",
  raw: "title: A plan\nowner: ana",
  fields: [
    { key: "title", value: { type: "text", text: "A plan" } },
    { key: "owner", value: { type: "text", text: "ana" } },
  ],
};

function hostWith(card: LockedHost["card"]): LockedHost {
  return {
    render: (node) => looks.get(node) ?? null,
    refresh: () => Promise.resolve(),
    blocks: { enhance: (element) => element, near: () => {} },
    card,
    // the editable properties take the card's place and edit through `replace`
    properties: (_target, _source, put) => {
      replace = put;
      return () => {};
    },
    reasonText: (reason) => `locked: ${reason}`,
    editLabel: "Source",
    edit: () => {},
    sourceLabel: "Source of this block",
    sourceHint: "Esc to finish",
    blockLabel: (name) => name,
    openHint: "Enter to edit",
  };
}

const viewCard: LockedHost["card"] = (target) => {
  const card = document.createElement("section");
  card.className = "card-stub";
  target.append(card);
  return () => card.remove();
};

beforeAll(async () => {
  const host = document.createElement("div");
  document.body.append(host);
  core = await createCore({ host, locked: hostWith(viewCard) });
});

afterAll(async () => {
  await core.destroy();
});

const TEXT = "---\ntitle: A plan\nowner: ana\n---\n\nBody words here.\n";
const SEGMENTS = [
  { lineStart: 1, lineEnd: 4, kind: "frontMatter", lock: "frontMatter" },
  { lineStart: 6, lineEnd: 6, kind: "paragraph", lock: null },
] as const;

/** TEXT in `editor` with View's card as the front matter's look. */
async function load(editor: Core): Promise<Built> {
  const built = await buildDoc(editor, TEXT, SEGMENTS);
  const front = built.doc.firstChild;
  if (front) looks.set(front, { kind: "frontMatter", frontMatter: FRONT });
  editor.setDoc(built.doc);
  return built;
}

describe("the front matter as editable properties", () => {
  it("replaces the block's source: the document gets those bytes and nothing else changes", async () => {
    looks = new WeakMap();
    const built = await load(core);
    const block = core.view.dom.querySelector<HTMLElement>(".pm-locked");
    const card = block?.querySelector(".card-stub");
    expect(block?.dataset.look).toBe("card");
    // untouched: the same text, no block changed
    expect(compose(core, built, core.view.state.doc).text).toBe(TEXT);
    card?.dispatchEvent(new MouseEvent("click", { bubbles: true, button: 0 }));
    expect(replace).not.toBeNull();
    replace?.("---\ntitle: A better plan\nowner: ana\n---");
    const composed = compose(core, built, core.view.state.doc);
    expect(composed.text).toBe(TEXT.replace("A plan", "A better plan"));
    expect(composed.changed).toBe(1);
  });

  it("a document that starts with the front matter opens with the caret in its first text, nothing selected", async () => {
    await load(core);
    const { selection, doc } = core.view.state;
    expect(selection).toBeInstanceOf(TextSelection);
    expect(selection.empty).toBe(true);
    expect(doc.resolve(selection.from).parent.textContent).toBe("Body words here.");
    expect(document.querySelector(".ProseMirror-selectednode")).toBeNull();
  });

  it("a front matter block with no card keeps its source and the reason", async () => {
    const plain = await createCore({
      host: document.body.appendChild(document.createElement("div")),
      locked: hostWith(() => null),
    });
    await load(plain);
    const block = plain.view.dom.querySelector<HTMLElement>(".pm-locked");
    expect(block?.dataset.look).toBe("source");
    expect(block?.querySelector(".pm-locked__source")?.textContent).toBe(
      "---\ntitle: A plan\nowner: ana\n---",
    );
    expect(block?.querySelector(".pm-locked__why")?.textContent).toBe("locked: frontMatter");
    await plain.destroy();
  });
});
