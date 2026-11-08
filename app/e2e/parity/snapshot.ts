// In-page collectors of the parity probe. They run through `page.evaluate`, which sends only the function's source:
// each one stands alone (no imports, no module helpers).

import type { Item, Props, Side } from "./compare.ts";

export type SideArg = { root: string; mode: "view" | "edit" };

/** Blocks, elements and computed styles under `root` (View: the reading body; Edit: the ProseMirror root). */
export async function sideOf(arg: SideArg): Promise<Side | null> {
  const root = document.querySelector(arg.root);
  if (!(root instanceof HTMLElement)) return null;

  // View loads pictures lazily: both modes must measure drawn pictures
  const pictures = [...root.querySelectorAll("img")];
  for (const img of pictures) img.loading = "eager";
  await Promise.all(
    pictures.map((img) =>
      img.complete
        ? null
        : new Promise<void>((done) => {
            img.addEventListener("load", () => done(), { once: true });
            img.addEventListener("error", () => done(), { once: true });
            setTimeout(done, 3_000);
          }),
    ),
  );

  const px = (n: number): string => `${Math.round(n * 100) / 100}px`;
  const one = (values: string[]): string =>
    values.every((v) => v === values[0]) ? (values[0] ?? "") : values.join(" ");
  const SIDES = ["top", "right", "bottom", "left"];
  const border = (s: CSSStyleDeclaration): string => {
    const parts = SIDES.map((k) =>
      s.getPropertyValue(`border-${k}-style`) === "none"
        ? "none"
        : `${s.getPropertyValue(`border-${k}-width`)} ${s.getPropertyValue(`border-${k}-style`)} ${s.getPropertyValue(`border-${k}-color`)}`,
    );
    return parts.every((p) => p === parts[0]) ? (parts[0] ?? "none") : parts.join(" / ");
  };
  const radius = (s: CSSStyleDeclaration): string =>
    one(
      ["top-left", "top-right", "bottom-right", "bottom-left"].map((c) =>
        s.getPropertyValue(`border-${c}-radius`),
      ),
    );
  const styleOf = (el: Element): Props => {
    const s = getComputedStyle(el);
    const line = s.getPropertyValue("text-decoration-line");
    return {
      "font-family": s.fontFamily,
      "font-size": s.fontSize,
      "font-weight": s.fontWeight,
      "line-height": s.lineHeight,
      color: s.color,
      "background-color": s.backgroundColor,
      "margin-top": s.marginTop,
      "margin-bottom": s.marginBottom,
      padding: one(SIDES.map((k) => s.getPropertyValue(`padding-${k}`))),
      border: border(s),
      "border-radius": radius(s),
      "text-decoration":
        line === "none"
          ? "none"
          : `${line} ${s.getPropertyValue("text-decoration-style")} ${s.getPropertyValue("text-decoration-color")}`,
    };
  };

  // decorations around the text (heading anchors, the code block's corner, the lock bar) do not name the block
  const NOT_TEXT = ".anchor, .corner, .lang, button, .pm-locked__bar";
  const textOf = (el: Element): string => {
    let out = "";
    const walker = document.createTreeWalker(el, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n !== null; n = walker.nextNode()) {
      if (n.parentElement?.closest(NOT_TEXT)) continue;
      out += n.textContent ?? "";
    }
    return out
      .toLowerCase()
      .replace(/[^\p{L}\p{N}]+/gu, "")
      .slice(0, 60);
  };

  const kindOf = (el: Element): string => {
    if (el.matches("details.prompt-block, .pm-prompt-frame")) return "prompt";
    if (el.classList.contains("pm-locked")) return "locked";
    if (el.matches(".code-block, pre")) return "code";
    if (el.matches(".table-wrap, .tableWrapper, table")) return "table";
    if (el.matches(".markdown-alert")) return "alert";
    if (el.matches("section.footnotes")) return "footnotes";
    return el.localName;
  };

  /** The box that draws a code block's | table's frame: the block itself or the code | table in it. */
  const frameOf = (el: Element): Props => {
    const framed = [el, el.querySelector(":scope > pre, :scope > table")].find(
      (x): x is Element => x !== null && getComputedStyle(x).borderTopStyle !== "none",
    );
    if (framed === undefined) {
      // what a box without a frame computes to, so only a real difference shows
      return {
        "frame-border": "none",
        "frame-radius": "0px",
        "frame-background": "rgba(0, 0, 0, 0)",
      };
    }
    const s = getComputedStyle(framed);
    return {
      "frame-border": border(s),
      "frame-radius": radius(s),
      "frame-background": s.backgroundColor,
    };
  };

  const listKind = (li: Element): string => {
    const list = li.parentElement;
    const task =
      li.classList.contains("task-list-item") || li.getAttribute("data-item-type") === "task";
    const kind = task ? "task" : list?.localName === "ol" ? "ordered" : "bullet";
    // ProseMirror wraps every item's text in a paragraph: Edit says loose by the list's `spread` (a list in a block
    // Edit draws as View is View's own)
    const loose =
      arg.mode === "edit" && li.closest(".pm-locked") === null
        ? list?.getAttribute("data-spread") === "true"
        : [...(list?.children ?? [])].some((c) => c.querySelector(":scope > p") !== null);
    return `li.${kind}.${loose ? "loose" : "tight"}`;
  };

  /** Distinct text colors in a code block: one = not highlighted. */
  const tokenColors = (pre: Element): number => {
    const colors = new Set<string>();
    const walker = document.createTreeWalker(pre, NodeFilter.SHOW_TEXT);
    for (let n = walker.nextNode(); n !== null; n = walker.nextNode()) {
      const parent = n.parentElement;
      if (parent !== null && (n.textContent ?? "").trim() !== "")
        colors.add(getComputedStyle(parent).color);
    }
    return colors.size;
  };

  const itemsOf = (block: Element): Item[] => {
    const out: Item[] = [];
    const box = block.getBoundingClientRect();
    for (const el of [block, ...block.querySelectorAll("*")]) {
      const tag = el.localName;
      const r = el.getBoundingClientRect();
      let type: string | null = null;
      let flavor = "";
      const extra: Props = {};
      if (el.classList.contains("lang")) {
        type = "code-label";
        extra["offset-top"] = px(r.top - box.top);
        extra["offset-right"] = px(box.right - r.right);
      } else if (/^h[1-6]$/.test(tag)) type = tag;
      else if (tag === "summary") type = "summary";
      else if (tag === "p") type = el === block ? "p" : null;
      else if (tag === "li") {
        type = listKind(el);
        extra["height"] = px(r.height);
        // a child's bottom margin collapses outside the item's box: the space shows only before the next item
        const next = el.nextElementSibling;
        if (next !== null && next.localName === "li") {
          extra["gap-after"] = px(next.getBoundingClientRect().top - r.bottom);
        }
      } else if (tag === "tr") {
        type = "tr";
        extra["height"] = px(r.height);
      } else if (["table", "th", "td", "blockquote", "hr"].includes(tag)) type = tag;
      else if (tag === "pre") {
        type = "pre";
        extra["token-colors"] = String(tokenColors(el));
      } else if (tag === "code") {
        if (el.closest("pre") === null) {
          type = "code";
          if (el.classList.contains("code-ref")) flavor = "code-ref";
        }
      } else if (tag === "a") {
        if (el.classList.contains("anchor")) type = "anchor";
        else if (el.classList.contains("at-ref")) type = "at-ref";
        else {
          type = "a";
          if (el.classList.contains("is-missing")) flavor = "missing";
        }
      } else if (["kbd", "sub", "mark"].includes(tag)) type = tag;
      // HTML written in a line (a footnote's number is its own `sup`); ProseMirror's own break in an empty line is not
      else if (tag === "sup" && !el.classList.contains("footnote-ref")) type = tag;
      else if (tag === "br" && !el.classList.contains("ProseMirror-trailingBreak")) type = tag;
      else if (tag === "span" && el.classList.contains("code-ref")) type = "code-ref";
      else if (tag === "span" && el.classList.contains("tvar")) type = "tvar";
      else if (tag === "img" && !el.classList.contains("ProseMirror-separator")) {
        type = "img";
        extra["width"] = px(r.width);
        extra["height"] = px(r.height);
      }
      if (type === null) continue;
      out.push({
        type,
        label: flavor === "" ? type : `${type}(${flavor})`,
        props: { ...styleOf(el), ...extra },
      });
    }
    return out;
  };

  const top = root.getBoundingClientRect().top;
  const blocks: Side["blocks"] = [];

  // the front matter card: above View's body, inside Edit's (a block kept as written); measured first on both sides,
  // its rows by their place in the card
  const card =
    arg.mode === "view"
      ? root.parentElement?.querySelector(":scope > section.card")
      : root.querySelector(':scope > .pm-locked[data-look="card"] > section.card');
  if (card instanceof HTMLElement) {
    const r = card.getBoundingClientRect();
    const items: Item[] = [];
    for (const el of card.querySelectorAll("dt, dd, .chip, pre")) {
      const e = el.getBoundingClientRect();
      const type = `card-${el.classList.contains("chip") ? "chip" : el.localName}`;
      items.push({
        type,
        label: type,
        props: {
          ...styleOf(el),
          "offset-left": px(e.left - r.left),
          "offset-top": px(e.top - r.top),
          height: px(e.height),
        },
      });
    }
    blocks.push({
      kind: "card",
      key: `card:${textOf(card)}`,
      top: r.top - top,
      bottom: r.bottom - top,
      props: { height: px(r.height), ...frameOf(card) },
      items,
      locked: null,
    });
  }

  // a block Edit keeps as written and draws as View: the blocks it holds are View's, measured as View's are; the
  // front matter card is measured above; one that shows its source stays one locked block.
  // A prompt box: the box itself (View's details | Edit's frame under its open tag line), its summary row, then the
  // blocks it holds (View's children | the editor's blocks between the tag lines); the close tag line is the box's edge.
  const shown: HTMLElement[] = [];
  const add = (el: Element): void => {
    if (!(el instanceof HTMLElement) || el.matches('.pm-locked[data-look="card"]')) return;
    if (el.matches("details.prompt-block")) {
      shown.push(el);
      for (const held of el.children) add(held);
    } else if (el.matches('.pm-locked[data-look="prompt"]')) {
      for (const part of [":scope > .pm-prompt-frame", ":scope > details > summary"]) {
        const found = el.querySelector(part);
        if (found !== null) add(found);
      }
    } else if (el.matches('.pm-locked[data-look="view"]')) {
      for (const held of el.children) add(held);
    } else shown.push(el);
  };
  for (const el of root.children) add(el);
  for (const el of shown) {
    // ProseMirror's own helpers (gap cursor, widgets) are editing affordances
    if (el.matches(".ProseMirror-gapcursor, .ProseMirror-widget")) continue;
    const r = el.getBoundingClientRect();
    // nothing drawn (a link definition in Edit, an empty block): nothing to compare
    if (r.height === 0) continue;
    const kind = kindOf(el);
    const locked =
      kind === "locked"
        ? { kind: el.dataset["kind"] ?? "other", reason: el.dataset["reason"] ?? "" }
        : null;
    // a prompt box is named by its tag and measured as a frame: what it holds is measured in its own blocks
    const box = kind === "prompt";
    blocks.push({
      kind,
      key: `${kind}:${box ? (el.dataset["tag"] ?? "") : locked === null ? textOf(el) : ""}`,
      top: r.top - top,
      bottom: r.bottom - top,
      props: {
        height: px(r.height),
        ...(kind === "code" || kind === "table" || box ? frameOf(el) : {}),
      },
      items: locked === null && !box ? itemsOf(el) : [],
      locked,
    });
  }
  const column = root.getBoundingClientRect();
  return { column: { left: px(column.left), width: px(column.width) }, blocks };
}

/**
 * The front matter as the reader sees it: View's card above the body | Edit's properties opened in place. Its text
 * (white space collapsed) and its chips; null: no front matter shown so.
 */
export function cardOf(arg: SideArg): { text: string; chips: number } | null {
  const root = document.querySelector(arg.root);
  const card =
    arg.mode === "view"
      ? root?.parentElement?.querySelector(":scope > section.card")
      : root?.querySelector(':scope > .pm-locked[data-look="properties"]');
  if (!(card instanceof HTMLElement)) return null;
  return {
    text: card.innerText.replace(/\s+/g, " ").trim(),
    chips: card.querySelectorAll(".chip").length,
  };
}

/** The document's scroll offset (View and Edit share the scroller). */
export function scrollOf(): number {
  return document.querySelector("[data-scroller]")?.scrollTop ?? 0;
}

/** Right after entering Edit: scroll offset, selected nodes, rows the toolbar takes. */
export function entryOf(): { scroll: number; selected: number; toolbarRows: number } {
  const editor = document.querySelector(".pm-editor");
  const toolbar = editor?.closest(".pane")?.querySelector('[role="toolbar"]');
  // items of one row share their vertical middle (the toolbar centers them); a wrapped item sits a row lower
  const middles = [...(toolbar?.children ?? [])]
    .map((c) => c.getBoundingClientRect())
    .filter((r) => r.width > 0 && r.height > 0)
    .map((r) => r.top + r.height / 2)
    .sort((a, b) => a - b);
  let rows = 0;
  let last = Number.NEGATIVE_INFINITY;
  for (const m of middles) {
    if (m - last > 4) rows += 1;
    last = m;
  }
  return {
    scroll: document.querySelector("[data-scroller]")?.scrollTop ?? 0,
    selected: document.querySelectorAll(".pm-editor .ProseMirror-selectednode").length,
    toolbarRows: rows,
  };
}

/** Screens of the document pane: scroll steps of one screen that reach its end. */
export function screensOf(): number {
  const pane = document.querySelector("[data-scroller]");
  if (!(pane instanceof HTMLElement) || pane.clientHeight === 0) return 0;
  return Math.ceil(pane.scrollHeight / pane.clientHeight);
}

/** Scrolls the document pane `screen` screens down (0: the top). */
export function scrollToScreen(screen: number): void {
  const pane = document.querySelector("[data-scroller]");
  if (pane instanceof HTMLElement) pane.scrollTop = screen * pane.clientHeight;
}
