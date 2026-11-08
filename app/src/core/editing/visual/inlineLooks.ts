// View's inline looks in Edit: links to missing files, code references (`src/main.rs:12`), heading anchors and, with
// the prompt-blocks plugin on, `{{variables}}` and `@file` references. Found by View's own rules (`doc/codeRefs.ts`,
// `doc/decorate.ts`) and the renderer's (`promptText.ts`), drawn with View's classes: decorations around the text, a
// widget for the anchor, and a class on the editor's own `a` | `code` element where View marks that element. The
// document never changes; the editor's own elements tell ProseMirror to ignore those class changes.
//
// A block is read again only when it changed (what it holds is cached per node); its previous looks are mapped
// through the edits until typing rests, then drawn again, never while a composition is open. What the backend must
// answer (where a link leads, which files a reference names, whether an `@file` exists) is asked once per key, then
// cached; a change of the project's files asks the known keys again and redraws only the blocks whose answers moved.

import { DOMSerializer, type Mark, type Node as PMNode } from "@milkdown/kit/prose/model";
import { Plugin, PluginKey, type Transaction } from "@milkdown/kit/prose/state";
import { AddMarkStep, AttrStep, RemoveMarkStep } from "@milkdown/kit/prose/transform";
import {
  Decoration,
  DecorationSet,
  type EditorView,
  type MarkViewConstructor,
} from "@milkdown/kit/prose/view";
import type { LinkTarget } from "../../../api";
import { findTextRefs, parseInlineRef, type CodeRef } from "../../doc/codeRefs";
import { CODE_REF, isLocalHref, type CodeRefTarget } from "../../doc/decorate";
import { promptPieces, type PromptPiece } from "./promptText";

/** How long typing rests before changed blocks are drawn again and new keys are asked. */
export const LOOKS_DELAY_MS = 150;

/** What the looks need from the app: View's lookups, its tooltips, and its way to open a reference. */
export type LooksHost = {
  /** Where each link of the document leads, in order (`null`: could not be asked). */
  links(hrefs: readonly string[]): Promise<readonly (LinkTarget | null)[]>;
  /** The project files each written code reference can mean (`null`: could not be asked). */
  codeRefs(written: readonly string[]): Promise<readonly (readonly string[])[] | null>;
  /** View marks `{{variables}}` and `@file` references in this document (the prompt-blocks plugin is on). */
  promptText(): boolean;
  refTitle(paths: readonly string[], line: number | null): string;
  missingTitle(path: string): string;
  /** Ctrl | Cmd + click on a reference: opened as View's click opens it (`at`: where, for a menu of several files). */
  openRef(target: CodeRefTarget, at: { x: number; y: number }): void;
  /** Runs `changed` when the project's files change. Returns what stops it. */
  onFiles(changed: () => void): () => void;
};

export const noLooks: LooksHost = {
  links: (hrefs) => Promise.resolve(hrefs.map(() => null)),
  codeRefs: () => Promise.resolve(null),
  promptText: () => false,
  refTitle: () => "",
  missingTitle: () => "",
  openRef: () => {},
  onFiles: () => () => {},
};

const LINK = "link";
const CODE = "inlineCode";
const HEADING = "heading";
const CODE_BLOCK = "code_block";
const MISSING = "is-missing";

type Span = { from: number; to: number };
type Ref = Span & { ref: CodeRef };
/** What a text block holds that may get a look (offsets in the block's content). */
type Found = {
  links: (Span & { href: string })[];
  /** Inline code that is a reference, whole. */
  inline: Ref[];
  /** References in plain text. */
  text: Ref[];
  prompt: PromptPiece[];
};

/** The looks that put a class on the editor's own element, and the ones a Ctrl | Cmd + click opens. */
type Look =
  | { look: "missing" }
  | { look: "codeRef"; target: CodeRefTarget }
  | { look: "textRef"; target: CodeRefTarget };

type Answers = {
  links: ReadonlyMap<string, LinkTarget>;
  refs: ReadonlyMap<string, readonly string[]>;
};

type Looks = {
  set: DecorationSet;
  /** Blocks changed since they were drawn (positions of the current document). */
  dirty: readonly Span[];
  answers: Answers;
  /** Keys to ask the backend at the next rest. */
  ask: { links: ReadonlySet<string>; refs: ReadonlySet<string> };
  /** No transaction since the document was set: the first answers are asked at once. */
  fresh: boolean;
};

type Meta =
  | { kind: "paint" }
  | {
      kind: "answers";
      links: readonly (readonly [string, LinkTarget | null])[];
      refs: readonly (readonly [string, readonly string[] | null])[];
    }
  | { kind: "files" };

const looksKey = new PluginKey<Looks>("PM_INLINE_LOOKS");

const EMPTY: Looks = {
  set: DecorationSet.empty,
  dirty: [],
  answers: { links: new Map(), refs: new Map() },
  ask: { links: new Set(), refs: new Set() },
  fresh: true,
};

function isMeta(value: unknown): value is Meta {
  return typeof value === "object" && value !== null && "kind" in value;
}

const isLine = (value: unknown): value is number | null =>
  value === null || typeof value === "number";

function isTarget(value: unknown): value is CodeRefTarget {
  return (
    typeof value === "object" &&
    value !== null &&
    "paths" in value &&
    Array.isArray(value.paths) &&
    value.paths.every((path) => typeof path === "string") &&
    "line" in value &&
    isLine(value.line) &&
    "col" in value &&
    isLine(value.col)
  );
}

/** The look a decoration of this plugin carries (its spec), or null for any other decoration. */
function lookOf(spec: unknown): Look | null {
  if (typeof spec !== "object" || spec === null || !("look" in spec)) return null;
  const look: unknown = spec.look;
  if (look === "missing") return { look };
  if ((look === "codeRef" || look === "textRef") && "target" in spec && isTarget(spec.target)) {
    return { look, target: spec.target };
  }
  return null;
}

// ---- what a block holds -----------------------------------------------------------------------------------------

const scans = new WeakMap<PMNode, Found>();

/** The pieces of plain text a line holds (View reads text node by text node; a line break ends one too). */
function scanText(text: string, offset: number, found: Found): void {
  let start = 0;
  for (const line of text.split("\n")) {
    const at = offset + start;
    for (const { start: from, end: to, ref } of findTextRefs(line)) {
      found.text.push({ from: at + from, to: at + to, ref });
    }
    for (const piece of promptPieces(line)) {
      found.prompt.push({ ...piece, from: at + piece.from, to: at + piece.to });
    }
    start += line.length + 1;
  }
}

/** Links (one per run of the same link), whole inline code spans outside links, plain text outside both. */
function scan(block: PMNode): Found {
  const cached = scans.get(block);
  if (cached) return cached;
  const found: Found = { links: [], inline: [], text: [], prompt: [] };
  let link: (Span & { href: string; mark: Mark }) | null = null;
  let code: (Span & { text: string }) | null = null;
  const endCode = (): void => {
    const ref = code === null ? null : parseInlineRef(code.text);
    if (code !== null && ref !== null) found.inline.push({ from: code.from, to: code.to, ref });
    code = null;
  };
  const endLink = (): void => {
    if (link !== null && isLocalHref(link.href)) found.links.push(link);
    link = null;
  };
  block.forEach((child, offset) => {
    const end = offset + child.nodeSize;
    const linked = child.marks.find((m) => m.type.name === LINK);
    if (link !== null && !(linked && linked.eq(link.mark))) endLink();
    if (linked) {
      if (link === null)
        link = { from: offset, to: end, href: String(linked.attrs.href ?? ""), mark: linked };
      else link.to = end;
    }
    const isCode = child.isText && child.marks.some((m) => m.type.name === CODE);
    if (isCode && !linked) {
      if (code === null) code = { from: offset, to: end, text: child.text ?? "" };
      else {
        code.to = end;
        code.text += child.text ?? "";
      }
    } else endCode();
    if (child.isText && !isCode && !linked) scanText(child.text ?? "", offset, found);
  });
  endCode();
  endLink();
  scans.set(block, found);
  return found;
}

// ---- what a block looks like ------------------------------------------------------------------------------------

function anchor(): HTMLElement {
  const element = document.createElement("a");
  element.className = "anchor";
  // drawn for the look only: the heading is read, the mark is not, and it takes no Tab stop
  element.setAttribute("aria-hidden", "true");
  element.textContent = "#";
  return element;
}

/** The `@file` names a file that exists (the link to it leads to that very file). */
function exists(target: LinkTarget, path: string): boolean {
  return (target.type === "doc" || target.type === "asset") && target.path === path;
}

const overlaps = (a: Span, b: Span): boolean => a.from < b.to && b.from < a.to;

function blockLooks(
  block: PMNode,
  pos: number,
  host: LooksHost,
  answers: Answers,
  ask: { links: Set<string>; refs: Set<string> },
): Decoration[] {
  const found = scan(block);
  const start = pos + 1;
  const out: Decoration[] = [];
  if (block.type.name === HEADING && block.textContent.trim() !== "") {
    out.push(
      Decoration.widget(start, anchor, {
        side: -1,
        marks: [],
        key: "heading-anchor",
        ignoreSelection: true,
      }),
    );
  }
  for (const link of found.links) {
    const target = answers.links.get(link.href);
    if (target === undefined) ask.links.add(link.href);
    else if (target.type === "missing") {
      const look: Look = { look: "missing" };
      const title = host.missingTitle(target.path);
      out.push(Decoration.inline(start + link.from, start + link.to, { title }, look));
    }
  }
  // View's code references never start inside an `@file` it links
  const files: Span[] = [];
  if (host.promptText()) {
    for (const piece of found.prompt) {
      const from = start + piece.from;
      const to = start + piece.to;
      if (piece.kind === "variable") {
        out.push(Decoration.inline(from, to, { nodeName: "span", class: "tvar" }));
        continue;
      }
      const href = `/${piece.path}`;
      const target = answers.links.get(href);
      if (target === undefined) ask.links.add(href);
      if (target !== undefined && !exists(target, piece.path)) continue;
      files.push(piece);
      if (target !== undefined)
        out.push(Decoration.inline(from, to, { nodeName: "a", class: "at-ref" }));
    }
  }
  const refLook = (look: "codeRef" | "textRef", { from, to, ref }: Ref): void => {
    const paths = answers.refs.get(ref.path);
    if (paths === undefined) ask.refs.add(ref.path);
    if (paths === undefined || paths.length === 0) return;
    const target: CodeRefTarget = { paths: [...paths], line: ref.line, col: ref.col };
    const title = host.refTitle(paths, ref.line);
    const attrs = look === "codeRef" ? { title } : { nodeName: "span", class: CODE_REF, title };
    out.push(Decoration.inline(start + from, start + to, attrs, { look, target }));
  };
  for (const ref of found.inline) refLook("codeRef", ref);
  for (const ref of found.text)
    if (!files.some((file) => overlaps(file, ref))) refLook("textRef", ref);
  return out;
}

/** Text blocks (not code blocks) of `doc` that `spans` touch, by position. */
function blocksIn(doc: PMNode, spans: readonly Span[]): Map<number, PMNode> {
  const found = new Map<number, PMNode>();
  const size = doc.content.size;
  for (const { from, to } of spans) {
    doc.nodesBetween(Math.max(0, from - 1), Math.min(size, to + 1), (node, pos) => {
      if (!node.isTextblock) return !node.isAtom;
      if (node.type.name !== CODE_BLOCK) found.set(pos, node);
      return false;
    });
  }
  return found;
}

function allBlocks(doc: PMNode): Map<number, PMNode> {
  return blocksIn(doc, [{ from: 0, to: doc.content.size }]);
}

/** `looks` with `blocks` drawn again. */
function paint(looks: Looks, doc: PMNode, blocks: Map<number, PMNode>, host: LooksHost): Looks {
  const ask = { links: new Set(looks.ask.links), refs: new Set(looks.ask.refs) };
  const old: Decoration[] = [];
  const drawn: Decoration[] = [];
  for (const [pos, block] of blocks) {
    const end = pos + block.nodeSize;
    old.push(...looks.set.find(pos, end).filter((d) => d.from >= pos && d.to <= end));
    drawn.push(...blockLooks(block, pos, host, looks.answers, ask));
  }
  return { ...looks, set: looks.set.remove(old).add(doc, drawn), ask };
}

/** The ranges `tr` changed, in its final document (a mark | attribute step moves no text: its own range). */
function changedSpans(tr: Transaction): Span[] {
  const spans: Span[] = [];
  tr.steps.forEach((step, i) => {
    const rest = tr.mapping.slice(i + 1);
    const add = (from: number, to: number): void => {
      spans.push({ from: rest.map(from, -1), to: rest.map(to, 1) });
    };
    if (step instanceof AddMarkStep || step instanceof RemoveMarkStep) add(step.from, step.to);
    else if (step instanceof AttrStep) add(step.pos, step.pos + 1);
    else step.getMap().forEach((_oldStart, _oldEnd, from, to) => add(from, to));
  });
  return spans;
}

/** The blocks that hold one of the keys whose answer moved. */
function blocksWith(doc: PMNode, links: Set<string>, refs: Set<string>): Map<number, PMNode> {
  const found = new Map<number, PMNode>();
  for (const [pos, block] of allBlocks(doc)) {
    const { links: held, inline, text, prompt } = scan(block);
    if (
      held.some((l) => links.has(l.href)) ||
      prompt.some((p) => p.kind === "file" && links.has(`/${p.path}`)) ||
      [...inline, ...text].some((r) => refs.has(r.ref.path))
    ) {
      found.set(pos, block);
    }
  }
  return found;
}

function answered(
  looks: Looks,
  doc: PMNode,
  meta: Meta & { kind: "answers" },
  host: LooksHost,
): Looks {
  const links = new Map(looks.answers.links);
  const refs = new Map(looks.answers.refs);
  const ask = { links: new Set(looks.ask.links), refs: new Set(looks.ask.refs) };
  const moved = { links: new Set<string>(), refs: new Set<string>() };
  for (const [href, target] of meta.links) {
    ask.links.delete(href);
    if (target === null || JSON.stringify(links.get(href)) === JSON.stringify(target)) continue;
    links.set(href, target);
    moved.links.add(href);
  }
  for (const [written, paths] of meta.refs) {
    ask.refs.delete(written);
    if (paths === null || JSON.stringify(refs.get(written)) === JSON.stringify(paths)) continue;
    refs.set(written, paths);
    moved.refs.add(written);
  }
  const next = { ...looks, answers: { links, refs }, ask };
  if (moved.links.size === 0 && moved.refs.size === 0) return next;
  return paint(next, doc, blocksWith(doc, moved.links, moved.refs), host);
}

// ---- the editor's own elements ----------------------------------------------------------------------------------

/** `a` and `code` as their marks draw them, telling ProseMirror that a class put on them is no edit. */
const lookedMark: MarkViewConstructor = (mark, _view, inline) => {
  const toDOM = mark.type.spec.toDOM;
  const { dom, contentDOM } = DOMSerializer.renderSpec(
    document,
    toDOM ? toDOM(mark, inline) : ["span", 0],
  );
  return {
    dom,
    contentDOM,
    ignoreMutation: (m) =>
      m.type === "attributes" && m.target === dom && m.attributeName === "class",
  };
};

/** The element of mark `tag` around the text at `pos`. */
function markElement(view: EditorView, pos: number, tag: string): Element | null {
  const { node, offset } = view.domAtPos(pos + 1, -1);
  const held = node instanceof Element ? (node.childNodes[offset - 1] ?? node) : node;
  const element = (held instanceof Element ? held : held.parentElement)?.closest(tag) ?? null;
  return element !== null && element !== view.dom && view.dom.contains(element) ? element : null;
}

/** Puts View's class on the links and inline code the looks mark; takes it off the ones no longer marked. */
function markClasses(view: EditorView, before: Map<Element, string>): Map<Element, string> {
  const now = new Map<Element, string>();
  const doc = view.state.doc;
  const set = looksKey.getState(view.state)?.set ?? DecorationSet.empty;
  for (const decoration of set.find(undefined, undefined, (spec) => lookOf(spec) !== null)) {
    const look = lookOf(decoration.spec);
    if (look === null || look.look === "textRef") continue;
    const [tag, name] = look.look === "missing" ? ["a", MISSING] : ["code", CODE_REF];
    const $from = doc.resolve(decoration.from);
    const start = $from.start();
    $from.parent.forEach((child, offset) => {
      const at = start + offset;
      if (!child.isText || at >= decoration.to || at + child.nodeSize <= decoration.from) return;
      const element = markElement(view, Math.max(at, decoration.from), tag);
      if (element) now.set(element, name);
    });
  }
  for (const [element, name] of before) {
    if (now.get(element) !== name) element.classList.remove(name);
  }
  for (const [element, name] of now) {
    if (!element.classList.contains(name)) element.classList.add(name);
  }
  return now;
}

// ---- the plugin -------------------------------------------------------------------------------------------------

/** View's inline looks, with what `host` answers (read when a block is drawn). */
export function inlineLooks(host: () => LooksHost): Plugin<Looks> {
  return new Plugin<Looks>({
    key: looksKey,
    state: {
      init: (_config, state) => paint(EMPTY, state.doc, allBlocks(state.doc), host()),
      apply: (tr, value) => {
        let looks: Looks = { ...value, fresh: false };
        if (tr.docChanged) {
          const map = (span: Span): Span => ({
            from: tr.mapping.map(span.from, -1),
            to: tr.mapping.map(span.to, 1),
          });
          looks = {
            ...looks,
            set: looks.set.map(tr.mapping, tr.doc),
            dirty: [...looks.dirty.map(map), ...changedSpans(tr)],
          };
        }
        const meta: unknown = tr.getMeta(looksKey);
        if (!isMeta(meta)) return looks;
        switch (meta.kind) {
          case "paint":
            return { ...paint(looks, tr.doc, blocksIn(tr.doc, looks.dirty), host()), dirty: [] };
          case "answers":
            return answered(looks, tr.doc, meta, host());
          case "files":
            // the known answers stay drawn until the new ones come
            return {
              ...looks,
              ask: {
                links: new Set([...looks.ask.links, ...looks.answers.links.keys()]),
                refs: new Set([...looks.ask.refs, ...looks.answers.refs.keys()]),
              },
            };
          default:
            return meta satisfies never;
        }
      },
    },
    props: {
      decorations: (state) => looksKey.getState(state)?.set ?? null,
      markViews: { [LINK]: lookedMark, [CODE]: lookedMark },
      handleClick: (view, pos, event) => {
        if (!(event.ctrlKey || event.metaKey)) return false;
        const element =
          event.target instanceof Element ? event.target.closest(`.${CODE_REF}`) : null;
        if (element === null || !view.dom.contains(element)) return false;
        const set = looksKey.getState(view.state)?.set ?? DecorationSet.empty;
        for (const decoration of set.find(pos, pos)) {
          const look = lookOf(decoration.spec);
          if (look === null || look.look === "missing") continue;
          host().openRef(look.target, { x: event.clientX, y: event.clientY });
          return true;
        }
        return false;
      },
    },
    view: (view) => {
      let timer: ReturnType<typeof setTimeout> | undefined;
      let gone = false;
      let classes = markClasses(view, new Map());
      const asking = { links: new Set<string>(), refs: new Set<string>() };
      /** Answers that came during a composition: delivered when it ends. */
      const waiting: Meta[] = [];
      const send = (meta: Meta): void => {
        view.dispatch(view.state.tr.setMeta(looksKey, meta).setMeta("addToHistory", false));
      };
      const schedule = (delay: number): void => {
        clearTimeout(timer);
        timer = setTimeout(round, delay);
      };
      const deliver = (meta: Meta): void => {
        if (gone) return;
        // redrawing the text under an open composition would break it
        if (view.composing) {
          waiting.push(meta);
          schedule(LOOKS_DELAY_MS);
        } else send(meta);
      };
      const askLinks = async (hrefs: string[]): Promise<void> => {
        for (const href of hrefs) asking.links.add(href);
        let targets: readonly (LinkTarget | null)[] = [];
        try {
          targets = await host().links(hrefs);
        } catch {
          // not asked: these links keep the look they had
        } finally {
          for (const href of hrefs) asking.links.delete(href);
        }
        deliver({
          kind: "answers",
          links: hrefs.map((h, at) => [h, targets[at] ?? null]),
          refs: [],
        });
      };
      const askRefs = async (written: string[]): Promise<void> => {
        for (const path of written) asking.refs.add(path);
        let files: readonly (readonly string[])[] | null = null;
        try {
          files = await host().codeRefs(written);
        } catch {
          // not asked: these references keep the look they had
        } finally {
          for (const path of written) asking.refs.delete(path);
        }
        deliver({
          kind: "answers",
          links: [],
          refs: written.map((w, at) => [w, files?.[at] ?? null]),
        });
      };
      function round(): void {
        timer = undefined;
        if (view.composing) {
          schedule(LOOKS_DELAY_MS);
          return;
        }
        for (const meta of waiting.splice(0)) send(meta);
        if ((looksKey.getState(view.state)?.dirty.length ?? 0) > 0) send({ kind: "paint" });
        const ask = looksKey.getState(view.state)?.ask;
        const links = [...(ask?.links ?? [])].filter((href) => !asking.links.has(href));
        const refs = [...(ask?.refs ?? [])].filter((path) => !asking.refs.has(path));
        if (links.length > 0) void askLinks(links);
        if (refs.length > 0) void askRefs(refs);
      }
      const stop = host().onFiles(() => deliver({ kind: "files" }));
      schedule(0);
      return {
        update: (next, previous) => {
          classes = markClasses(next, classes);
          const looks = looksKey.getState(next.state);
          if (!looks) return;
          if (next.state.doc !== previous.doc) {
            schedule(looks.fresh ? 0 : LOOKS_DELAY_MS);
            return;
          }
          const work = looks.dirty.length + looks.ask.links.size + looks.ask.refs.size > 0;
          if (work && timer === undefined && looks !== looksKey.getState(previous)) schedule(0);
        },
        destroy: () => {
          gone = true;
          clearTimeout(timer);
          stop();
        },
      };
    },
  });
}
