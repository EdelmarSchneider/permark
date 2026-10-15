// Visual mode as one object: the Milkdown editor, the document it holds and the questions the rest of the app asks it
// (is it dirty, what would a save write, which block is the caret in). No Svelte in here; the components in
// `components/` draw the toolbar, menus and popovers around it.

import {
  imageAttr,
  imageSchema,
  inlineCodeKeymap,
  blockquoteKeymap,
} from "@milkdown/kit/preset/commonmark";
import { strikethroughKeymap } from "@milkdown/kit/preset/gfm";
import type { Node as PMNode } from "@milkdown/kit/prose/model";
import { EditorState, NodeSelection, Selection, type Transaction } from "@milkdown/kit/prose/state";
import type { EditorView } from "@milkdown/kit/prose/view";
import { SELECTION_READ_LIMIT, type SelectedText } from "../../agentContext";
import type { HeadingInfo } from "../../outline";
import { currentHeading } from "../../outline";
import { joinBlockMap, type BlockMap } from "../blockModel";
import { showsAtTop, watchViewport } from "../viewport";
import {
  analyze,
  buildDoc,
  composeDoc,
  isEmptyParagraph,
  lineOfNode,
  originOf,
  type Analysis,
  type Built,
  type LoadReport,
} from "./doc";
import { createCore, LOCKED, TM_BLOCK, type Core, type LockedHost } from "./core";
import { formatState, insertImage, runCommand, setCodeLanguage } from "./commands";
import { NO_FORMAT, type CommandId, type FormatState } from "./format";
import { applyLink, linkTarget, type LinkTarget } from "./link";
import { appPlugins, type PluginHost } from "./plugins";

/** The slash menu is open: what has been typed after the `/`, and where the caret is on screen. */
export type SlashState = { query: string; rect: DOMRect | null };

/** What Visual mode tells the rest of the app. */
export type VisualHandlers = {
  /** The document or the selection changed: read `status()` and `format()` again. */
  changed(): void;
  /** Images pasted or dropped on the editor. */
  files(files: File[], source: "paste" | "drop"): void;
  /** The button of a locked atom (or Enter on it): open Source at this block of the loaded text (null: a new block). */
  editInSource(block: number | null): void;
  /** Ctrl+K. */
  link(): void;
  /** The slash menu opened (with its state), changed, or closed (null). */
  slash(state: SlashState | null): void;
  /** The slash menu is open and wants this key (Up, Down, Enter). */
  slashKey(key: string): boolean;
};

export type VisualEditorOptions = {
  host: HTMLElement;
  label: string;
  locked: LockedHost;
  handlers: VisualHandlers;
  /** The URL an `<img>` can load for the `src` written in the Markdown (a vault-relative path, or a remote URL as it is). */
  imageUrl(src: string): string;
  /** Scroll the caret into view when it is placed. Off in tests, where there is no layout. */
  scroll?: boolean;
  /** The element that scrolls the document: the Outline reads where the view is from it, and scrolls it to a heading. */
  scroller?: HTMLElement;
};

/** What a load needs: the text being edited (as its blocks), and how it relates to the saved file. */
export type VisualInput = {
  map: BlockMap;
  /** Per block of `map`: it differs from the saved file (it comes from unsaved Source edits). */
  unsaved?: readonly boolean[];
  /** The text differs from the saved file (even when no block does: blank lines, say). */
  dirty?: boolean;
  /** The block to put the caret in (0-based, of `map`). */
  block?: number | null;
  /** Put the caret at the end of that block instead of its start. */
  atEnd?: boolean;
  /** Where the caret's block goes on screen: `start` puts it at the top (entering from the reading view keeps the place), the default only makes it visible. */
  reveal?: "start" | "nearest";
  /** The block (0-based, of `map`) to put at the top of the pane, whichever block the caret is in: the place the other mode was showing. */
  top?: number | null;
};

export type VisualStatus = {
  /** A save would write something other than the saved file. */
  dirty: boolean;
  /** How many blocks carry the unsaved-block bar. */
  changed: number;
};

/** A top-level heading of the document, and where its node starts. */
type DocHeading = HeadingInfo & { pos: number };

const EMPTY_ANALYSIS: Analysis = { nodes: [], structure: false, changed: 0, touched: 0 };

export class VisualEditor {
  readonly mode = "visual" as const;
  private built: Built | null = null;
  private initialDirty = false;
  /** The text that was saved last (see `rebase`); null until the document has been saved since it was loaded. */
  private savedText: string | null = null;
  /**
   * The nodes of the document when a save could not give them the blocks of the saved text: they are what the file holds, so no
   * unsaved-block bar is drawn for them (and the document is judged by its text alone). Null when the blocks were renumbered.
   */
  private savedNodes: WeakSet<PMNode> | null = null;
  /** The text of the document as it was last composed, for the document and the blocks it was composed from. */
  private composed: { doc: PMNode; built: Built; text: string } | null = null;
  private cache = new WeakMap<PMNode, Analysis>();
  private readonly headingCache = new WeakMap<PMNode, DocHeading[]>();
  private slashOpen = false;
  private slashDismissed = false;
  private destroyed = false;

  private constructor(
    readonly core: Core,
    private readonly options: VisualEditorOptions,
  ) {}

  static async create(options: VisualEditorOptions): Promise<VisualEditor> {
    let editor: VisualEditor | null = null;
    const host: PluginHost = {
      analysisFor: (doc) => editor?.analysisFor(doc) ?? null,
      updated: (view) => editor?.viewUpdated(view),
      files: (files, source) => options.handlers.files(files, source),
      link: () => options.handlers.link(),
      editInSource: (block) => options.handlers.editInSource(block),
      slashKey: (key) => editor?.slashOpen === true && options.handlers.slashKey(key),
    };
    const core = await createCore({
      host: options.host,
      locked: options.locked,
      label: options.label,
      firstPlugins: appPlugins(host),
      config: (ctx) => {
        // the key remaps of docs/design/v0.2.md section 11: Ctrl+E always toggles editing, so inline code moves to Ctrl+`
        ctx.update(inlineCodeKeymap.key, (keys) => ({
          ...keys,
          ToggleInlineCode: { shortcuts: "Mod-`" },
        }));
        ctx.update(strikethroughKeymap.key, (keys) => ({
          ...keys,
          ToggleStrikethrough: { shortcuts: "Mod-Shift-x" },
        }));
        ctx.update(blockquoteKeymap.key, (keys) => ({
          ...keys,
          WrapInBlockquote: { shortcuts: ["Mod-Shift-.", "Mod->"] },
        }));
        // a relative image path is drawn from the app's own image scheme; the Markdown keeps the path as written
        ctx.update(imageSchema.key, (previous) => (c) => {
          const spec = previous(c);
          return {
            ...spec,
            parseDOM: [
              {
                tag: "img[src]",
                getAttrs: (dom: HTMLElement | string) => {
                  if (typeof dom === "string") return false;
                  return {
                    src: dom.getAttribute("data-src") ?? dom.getAttribute("src") ?? "",
                    alt: dom.getAttribute("alt") ?? "",
                    title: dom.getAttribute("title") ?? "",
                  };
                },
              },
            ],
            toDOM: (node: PMNode) => [
              "img",
              {
                ...c.get(imageAttr.key)(node),
                src: options.imageUrl(String(node.attrs.src)),
                "data-src": String(node.attrs.src),
                alt: String(node.attrs.alt),
                title: String(node.attrs.title),
              },
            ],
          };
        });
      },
    });
    editor = new VisualEditor(core, options);
    const dom = core.view.dom;
    dom.classList.add("md", "md-body", "tm-visual");
    dom.setAttribute("aria-label", options.label);
    dom.setAttribute("aria-multiline", "true");
    dom.setAttribute("role", "textbox");
    return editor;
  }

  // ---- the document ---------------------------------------------------------------------------------

  /** Replaces the document with the blocks of a text. Undo history starts over. */
  load(input: VisualInput): LoadReport {
    const built = buildDoc(this.core, { map: input.map, unsaved: input.unsaved });
    this.built = built;
    this.initialDirty = input.dirty ?? false;
    this.savedText = null;
    this.savedNodes = null;
    const { view } = this.core;
    view.updateState(
      EditorState.create({
        schema: view.state.schema,
        doc: built.doc,
        plugins: view.state.plugins,
      }),
    );
    if (input.block !== undefined && input.block !== null)
      this.placeCaret(input.block, input.atEnd ?? false, input.reveal ?? "nearest", input.top);
    this.options.handlers.changed();
    return built.report;
  }

  /** Goes back to the document as loaded: the edits and the undo history are dropped (the blocks are not read again). */
  restart(): void {
    if (!this.built) return;
    this.savedText = null;
    this.savedNodes = null;
    const { view } = this.core;
    view.updateState(
      EditorState.create({
        schema: view.state.schema,
        doc: this.built.doc,
        plugins: view.state.plugins,
      }),
    );
    this.options.handlers.changed();
  }

  private analysisFor(doc: PMNode): Analysis | null {
    if (!this.built) return null;
    let analysis = this.cache.get(doc);
    if (!analysis) {
      analysis = analyze(this.built, doc, this.savedNodes);
      // A document that writes the text that was saved has nothing unsaved, whatever its nodes say (a space at the end of a
      // paragraph that was typed and taken back leaves a node that is not the one that was saved, and the same text).
      if (
        this.savedText !== null &&
        (analysis.changed > 0 || analysis.touched > 0 || analysis.structure) &&
        this.composedText(doc) === this.savedText
      ) {
        analysis = {
          ...analysis,
          nodes: analysis.nodes.map((info) => (info.unsaved ? { ...info, unsaved: false } : info)),
          structure: false,
          changed: 0,
          touched: 0,
        };
      }
      this.cache.set(doc, analysis);
    }
    return analysis;
  }

  /** The text `doc` writes (the document the editor shows, or one it is asked about). */
  private composedText(doc: PMNode): string {
    if (!this.built) return "";
    const known = this.composed;
    if (known && known.doc === doc && known.built === this.built) return known.text;
    const text = composeDoc(this.core, this.built, doc).text;
    this.composed = { doc, built: this.built, text };
    return text;
  }

  private analysis(): Analysis {
    return this.analysisFor(this.core.view.state.doc) ?? EMPTY_ANALYSIS;
  }

  /** Whether a save would write something other than the saved file, and how many blocks carry the unsaved bar. */
  status(): VisualStatus {
    const a = this.analysis();
    let dirty = this.initialDirty || a.structure || a.touched > 0;
    // Once it has been saved, what matters is the text: the nodes may be edited and edited back to another node of the same text, and
    // when the blocks could not be renumbered they say nothing about what the file holds.
    if (this.savedText !== null && (dirty || this.savedNodes !== null)) {
      dirty = this.text() !== this.savedText;
    }
    return { dirty, changed: a.changed };
  }

  /** The file text a save would write: untouched blocks and separators as they were, edited blocks as the editor writes them. */
  text(): string {
    return this.composedText(this.core.view.state.doc);
  }

  /** The selected text, or null when nothing is selected. */
  selection(): SelectedText | null {
    const { doc, selection } = this.core.view.state;
    if (selection.empty) return null;
    // an image shows its alt text, a line break a space; only what an agent can be told is read
    const text = doc.textBetween(
      selection.from,
      Math.min(selection.to, selection.from + SELECTION_READ_LIMIT),
      "\n",
      (leaf) => (leaf.type.name === "image" ? String(leaf.attrs.alt ?? "") : " "),
    );
    return { text };
  }

  /** The 0-based line of the caret's block in `text()`. */
  caretLine(): number {
    if (!this.built) return 0;
    const composed = composeDoc(this.core, this.built, this.core.view.state.doc);
    return lineOfNode(composed, this.analysis(), this.caretChild());
  }

  /** The 0-based line, in `text()`, of the block at the top of the pane (the caret's block when there is no layout to look at). */
  topLine(): number {
    const { scroller } = this.options;
    if (!this.built || !scroller) return this.caretLine();
    const { view } = this.core;
    const edge = scroller.getBoundingClientRect().top;
    const { doc } = view.state;
    let child = Math.max(0, doc.childCount - 1);
    let found = false;
    doc.forEach((_node, pos, index) => {
      if (found) return;
      const dom = view.nodeDOM(pos);
      if (!(dom instanceof HTMLElement)) return;
      const box = dom.getBoundingClientRect();
      if (showsAtTop(box.top, box.bottom, edge)) {
        child = index;
        found = true;
      }
    });
    return lineOfNode(composeDoc(this.core, this.built, doc), this.analysis(), child);
  }

  /** The 0-based line, in `text()`, where a block of the loaded text starts (null: the block the caret is in). */
  lineOfBlock(block: number | null): number {
    if (!this.built) return 0;
    const analysis = this.analysis();
    let child = this.caretChild();
    if (block !== null) {
      const found = analysis.nodes.findIndex((n) => n.origin === block);
      if (found >= 0) child = found;
    }
    return lineOfNode(composeDoc(this.core, this.built, this.core.view.state.doc), analysis, child);
  }

  /**
   * Goes on from the blocks of the text that was just saved: the unsaved-block bar clears and the next save compares with what was
   * written. Typing never stops for it, so the document is left alone: its top-level nodes are the blocks of the saved text (they are
   * given the new block numbers, outside the undo history), and the focus, the selection and Ctrl+Z all stay as they were. Only when
   * the nodes are not the ones the saved text makes (the editor writes something that reads back differently) the document is loaded
   * again, keeping the focus and the selection but not the undo history.
   */
  rebase(map: BlockMap): void {
    const { view } = this.core;
    const next = buildDoc(this.core, { map });
    const tr = this.renumbering(next);
    this.initialDirty = false;
    // what was saved: the text the document is compared with from now on (it is what the file holds)
    this.savedText = joinBlockMap(map);
    this.cache = new WeakMap();
    if (tr) {
      this.built = next;
      this.savedNodes = null;
      view.dispatch(tr.setMeta("addToHistory", false));
      return;
    }
    // The nodes are not the blocks of the saved text (the editor wrote something that reads back as another number of blocks).
    // Even then the document is not loaded again: it stays as the person has it, the blocks it was made from stay too, its nodes
    // are what was saved (no unsaved-block bar for them), and what was saved is the text it is compared with.
    const saved = new WeakSet<PMNode>();
    view.state.doc.forEach((node) => saved.add(node));
    this.savedNodes = saved;
    this.options.handlers.changed();
  }

  /**
   * What gives the nodes of the document the block numbers of the saved text, or null when the nodes are not its blocks. The written
   * nodes, in order, are the blocks (an empty paragraph is not written and belongs to none), and the text just saved was written
   * from them, so that is all that has to hold: the nodes the person has are what they see and are kept, even where the saved
   * text reads back a little differently (a space at the end of a paragraph, which Markdown drops). Each written node becomes the
   * original of its block, so that it counts as saved and the next save writes the block's own bytes until it is edited.
   */
  private renumbering(next: Built): Transaction | null {
    const { state } = this.core.view;
    const tr = state.tr;
    const originals: PMNode[] = [];
    state.doc.forEach((node, pos) => {
      if (isEmptyParagraph(node)) {
        if (originOf(node) !== null) tr.setNodeAttribute(pos, TM_BLOCK, null);
        return;
      }
      const block = originals.length;
      originals.push(
        node.type.create({ ...node.attrs, [TM_BLOCK]: block }, node.content, node.marks),
      );
      if (originOf(node) !== block) tr.setNodeAttribute(pos, TM_BLOCK, block);
    });
    if (originals.length !== next.originals.length) return null;
    next.originals.splice(0, next.originals.length, ...originals);
    return tr;
  }

  /** The top-level node the selection is in. */
  private caretChild(): number {
    const { selection, doc } = this.core.view.state;
    const index =
      selection instanceof NodeSelection ? selection.$from.index(0) : selection.$head.index(0);
    return Math.min(index, Math.max(0, doc.childCount - 1));
  }

  /** The block (of the loaded text) the caret is in, or null in a block that was added. */
  caretBlock(): number | null {
    const analysis = this.analysis();
    for (let i = this.caretChild(); i >= 0; i--) {
      const origin = analysis.nodes[i]?.origin ?? null;
      if (origin !== null) return origin;
    }
    return null;
  }

  private placeCaret(
    block: number,
    atEnd: boolean,
    reveal: "start" | "nearest",
    top?: number | null,
  ): void {
    const { view } = this.core;
    const { doc } = view.state;
    const analysis = this.analysis();
    let index = analysis.nodes.findIndex((n) => n.origin === block);
    if (index < 0) index = Math.min(block, doc.childCount - 1);
    const startOf = (i: number): number => {
      let at = 0;
      for (let k = 0; k < i; k++) at += doc.child(k).nodeSize;
      return at;
    };
    // the block asked for (`top`, else the caret's) is the one that goes to the top of the pane, even when the caret has to be in
    // another one; when only front matter is above it (the reader shows that as a card over the text) the pane stays at the very top
    let topIndex = index;
    if (top !== undefined && top !== null) {
      const found = analysis.nodes.findIndex((n) => n.origin === top);
      topIndex = found >= 0 ? found : Math.min(top, doc.childCount - 1);
    }
    let revealIndex = topIndex;
    while (revealIndex > 0 && doc.child(revealIndex - 1).attrs.kind === "frontmatter")
      revealIndex--;
    if (revealIndex > 0) revealIndex = topIndex;
    const revealAt = startOf(revealIndex);
    // a locked block takes no caret: it goes to the next block that can be typed in (else the one before)
    const editable = (i: number): boolean => doc.child(i).type.name !== LOCKED;
    if (!editable(index)) {
      let next = index;
      while (next < doc.childCount && !editable(next)) next++;
      if (next >= doc.childCount) {
        next = index;
        while (next >= 0 && !editable(next)) next--;
      }
      if (next >= 0 && next < doc.childCount) {
        index = next;
        atEnd = false;
      }
    }
    const pos = startOf(index);
    const node = doc.child(index);
    let selection: Selection;
    if (node.type.name === LOCKED) selection = NodeSelection.create(doc, pos);
    else if (atEnd) selection = Selection.near(doc.resolve(pos + node.nodeSize - 1), -1);
    else selection = Selection.near(doc.resolve(pos + 1), 1);
    const tr = view.state.tr.setSelection(selection);
    if (this.options.scroll !== false) tr.scrollIntoView();
    view.dispatch(tr);
    if ((reveal === "start" || (top ?? null) !== null) && this.options.scroll !== false) {
      const dom = view.nodeDOM(revealAt);
      if (dom instanceof HTMLElement) this.scrollToTop(dom);
    }
  }

  /** Scrolls the pane so `el` is 8 px under its top edge. */
  private scrollToTop(el: HTMLElement): void {
    const { scroller } = this.options;
    if (!scroller) {
      el.scrollIntoView({ block: "start" });
      return;
    }
    const delta = el.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
    scroller.scrollTo({ top: Math.max(0, scroller.scrollTop + delta - 8), behavior: "instant" });
  }

  // ---- the Outline -----------------------------------------------------------------------------------------

  /** The top-level headings of a document (one inside a list or a quote is not listed, as in the reading view). */
  private headingList(doc: PMNode): DocHeading[] {
    let list = this.headingCache.get(doc);
    if (!list) {
      const found: DocHeading[] = [];
      doc.forEach((node, pos) => {
        if (node.type.name !== "heading") return;
        // an image shows its alt text, a line break a space
        const text = node.textBetween(0, node.content.size, " ", (leaf) =>
          leaf.type.name === "image" ? String(leaf.attrs.alt ?? "") : " ",
        );
        found.push({
          level: Number(node.attrs.level) || 1,
          text: text.replace(/\s+/g, " ").trim(),
          pos,
        });
      });
      list = found;
      this.headingCache.set(doc, list);
    }
    return list;
  }

  headings(): HeadingInfo[] {
    return this.headingList(this.core.view.state.doc).map(({ level, text }) => ({ level, text }));
  }

  private headingEl(heading: DocHeading): HTMLElement | null {
    const dom = this.core.view.nodeDOM(heading.pos);
    return dom instanceof HTMLElement ? dom : null;
  }

  /** Scrolls heading `index` to 8 px under the top of the editor. The selection stays where it is. */
  scrollToHeading(index: number): void {
    const heading = this.headingList(this.core.view.state.doc)[index];
    const el = heading ? this.headingEl(heading) : null;
    if (!this.options.scroller || !el) return;
    this.scrollToTop(el);
  }

  /** The heading of the section in view: the last one at or above the top-third line of the editor. */
  currentHeading(): number | null {
    const { scroller } = this.options;
    const list = this.headingList(this.core.view.state.doc);
    if (!scroller || list.length === 0) return null;
    const top = scroller.getBoundingClientRect().top;
    const height = scroller.clientHeight;
    const atBottom = scroller.scrollTop + height >= scroller.scrollHeight - 2;
    return currentHeading(
      list.length,
      (i) => (this.headingEl(list[i])?.getBoundingClientRect().top ?? top) - top,
      { line: height / 3, bottomEdge: atBottom ? height : undefined },
    );
  }

  /** Calls `handler` when the editor scrolls, and with `true` when the person takes over. Returns the function that stops it. */
  onViewport(handler: (byUser: boolean) => void): () => void {
    const { scroller } = this.options;
    return scroller ? watchViewport(scroller, handler) : () => {};
  }

  // ---- reading the selection --------------------------------------------------------------------------

  format(): FormatState {
    return this.destroyed ? NO_FORMAT : formatState(this.core.view.state);
  }

  run(command: CommandId): boolean {
    return runCommand(this.core, command);
  }

  setLanguage(language: string): boolean {
    return setCodeLanguage(this.core, language);
  }

  focus(): void {
    this.core.view.focus();
  }

  /** Puts the caret at the end of the document (a click on the empty space below it). */
  focusEnd(): void {
    const { view } = this.core;
    view.dispatch(view.state.tr.setSelection(Selection.atEnd(view.state.doc)));
    view.focus();
  }

  /** Where the caret is on screen, or null when it cannot be told. */
  caretRect(): DOMRect | null {
    try {
      const at = this.core.view.coordsAtPos(this.core.view.state.selection.head);
      return new DOMRect(at.left, at.top, 1, at.bottom - at.top);
    } catch {
      return null;
    }
  }

  /** The person closed the slash menu with Esc: it stays closed until the `/` is gone. */
  dismissSlash(): void {
    if (this.slashOpen) this.slashDismissed = true;
  }

  insertImage(src: string, alt = ""): boolean {
    return insertImage(this.core, src, alt);
  }

  // ---- links ---------------------------------------------------------------------------------------------

  linkTarget(): LinkTarget {
    return linkTarget(this.core.view.state);
  }

  applyLink(target: LinkTarget, next: { text: string; href: string }): boolean {
    return applyLink(this.core.view, target, next);
  }

  // ---- the slash menu --------------------------------------------------------------------------------------

  /** The `/query` the caret is at the end of, in an otherwise empty paragraph. */
  private slashState(view: EditorView): SlashState | null {
    const { selection } = view.state;
    if (!selection.empty) return null;
    const { $from } = selection;
    const parent = $from.parent;
    if (parent.type.name !== "paragraph" || $from.parentOffset !== parent.content.size) return null;
    for (let depth = $from.depth; depth > 0; depth--) {
      if ($from.node(depth).type.name.startsWith("table")) return null;
    }
    const match = /^\/(\S*)$/.exec(parent.textContent);
    if (!match) return null;
    let rect: DOMRect | null = null;
    try {
      const at = view.coordsAtPos($from.pos);
      rect = new DOMRect(at.left, at.top, 1, at.bottom - at.top);
    } catch {
      // no layout (tests): the menu has nowhere to go, the state is still right
    }
    return { query: match[1], rect };
  }

  /** Removes the `/query` and runs a command in its place. */
  runSlash(command: CommandId | "image"): void {
    const { view } = this.core;
    const { $from } = view.state.selection;
    if ($from.parent.type.name === "paragraph" && /^\//.test($from.parent.textContent)) {
      view.dispatch(view.state.tr.delete($from.start(), $from.pos));
    }
    if (command !== "image") runCommand(this.core, command);
    else view.focus();
  }

  private viewUpdated(view: EditorView): void {
    if (this.destroyed) return;
    let slash = this.slashState(view);
    if (slash === null) this.slashDismissed = false;
    else if (this.slashDismissed) slash = null;
    const was = this.slashOpen;
    this.slashOpen = slash !== null;
    if (slash || was) this.options.handlers.slash(slash);
    this.options.handlers.changed();
  }

  // ---- lifetime -----------------------------------------------------------------------------------------------

  /** The block map of the loaded text, for the store (it maps the caret between modes). */
  get map(): BlockMap | null {
    return this.built?.map ?? null;
  }

  async destroy(): Promise<void> {
    this.destroyed = true;
    await this.core.destroy();
  }

  /** Which original block a top-level node stands for (tests and the store use this to follow a block). */
  static originOf = originOf;
}
