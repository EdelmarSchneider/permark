// Work a block does when it comes near the screen: embeds (code highlighting, formulas, diagrams) are mounted,
// links are looked up (a missing target is marked), code references become links to the editor.
// Nothing here runs for blocks far from the screen, so a long document opens at once.

import { mount, unmount, type Component } from "svelte";
import type { Api, LinkTarget } from "../../api";
import { t } from "../i18n";
import type { EmbedProps } from "../plugins/props";
import { findTextRefs, parseInlineRef, refLabel, type CodeRef } from "./codeRefs";

export type LazyEmbed = () => Promise<{ default: Component<EmbedProps> }>;
/** The embed for a language word (`mermaid`) or a reserved name (`@code`, `@math`), if a plugin brings one. */
export type EmbedLookup = (language: string) => LazyEmbed | undefined;

/** The mark on an element that holds a code reference. */
export const CODE_REF = "code-ref";

const SCHEME = /^[a-z][a-z0-9+.-]*:/i;
const BATCH_AFTER_MS = 0;

type Mounted = {
  loader: LazyEmbed;
  original: Node[];
  className: string;
  instance: Record<string, unknown> | null;
  cancelled: boolean;
};

/** Where links lead, per document: asked in batches, remembered. */
export class LinkCache {
  #api: Api;
  #doc: string | null = null;
  #known = new Map<string, LinkTarget>();
  #queue = new Set<string>();
  #flush: Promise<void> | null = null;

  constructor(api: Api) {
    this.#api = api;
  }

  /** Forgets everything when another document is shown. */
  forDocument(path: string | null): void {
    if (path === this.#doc) return;
    this.#doc = path;
    this.#known.clear();
    this.#queue.clear();
  }

  peek(href: string): LinkTarget | undefined {
    return this.#known.get(href);
  }

  /** Targets of `hrefs`, in order (`null`: could not be asked). Requests of one moment go out as one. */
  async resolve(hrefs: readonly string[]): Promise<(LinkTarget | null)[]> {
    for (const href of hrefs) {
      if (!this.#known.has(href)) this.#queue.add(href);
    }
    if (this.#queue.size > 0) this.#flush ??= this.#send();
    await this.#flush;
    return hrefs.map((href) => this.#known.get(href) ?? null);
  }

  async #send(): Promise<void> {
    await new Promise((resolve) => setTimeout(resolve, BATCH_AFTER_MS));
    const doc = this.#doc;
    const asked = [...this.#queue];
    this.#queue.clear();
    this.#flush = null;
    if (doc === null || asked.length === 0) return;
    const result = await this.#api.resolveLinks(doc, asked);
    if (!result.ok || doc !== this.#doc) return;
    asked.forEach((href, at) => {
      const target = result.value[at];
      if (target) this.#known.set(href, target);
    });
  }
}

function isLocalHref(href: string | null): href is string {
  return href !== null && href !== "" && !href.startsWith("#") && !SCHEME.test(href);
}

export class Decorator {
  readonly #api: Api;
  readonly links: LinkCache;
  #doc: string | null = null;
  #embeds: EmbedLookup = () => undefined;
  #mounted = new Map<HTMLElement, Mounted>();
  /** Blocks that are near the screen (and so have their embeds). */
  #near = new Set<HTMLElement>();

  constructor(api: Api) {
    this.#api = api;
    this.links = new LinkCache(api);
  }

  setDocument(path: string | null): void {
    this.#doc = path;
    this.links.forDocument(path);
  }

  /** The embeds on offer changed (a plugin came or went): mount the new ones, take down the ones that left. */
  setEmbeds(lookup: EmbedLookup): void {
    this.#embeds = lookup;
    for (const block of this.#near) this.#syncEmbeds(block);
  }

  /** A block came near the screen. Returns what takes its embeds down. */
  near(block: HTMLElement): () => void {
    this.#near.add(block);
    this.#syncEmbeds(block);
    this.#decorateLinks(block);
    void this.#decorateCodeRefs(block);
    return () => {
      this.#near.delete(block);
      for (const element of this.#placeholders(block)) this.#unmountOne(element);
    };
  }

  /** Takes every embed down (the document is closed). */
  dispose(): void {
    for (const element of [...this.#mounted.keys()]) this.#unmountOne(element);
    this.#near.clear();
  }

  // ---- embeds ------------------------------------------------------------------------------------------------

  #placeholders(block: HTMLElement): HTMLElement[] {
    const found = Array.from(block.querySelectorAll<HTMLElement>(".code-block, [data-math-style]"));
    if (block.matches(".code-block, [data-math-style]")) found.unshift(block);
    return found;
  }

  #lookup(element: HTMLElement): LazyEmbed | undefined {
    if (element.hasAttribute("data-math-style")) return this.#embeds("@math");
    const language = (element.getAttribute("data-info") ?? "").toLowerCase();
    return (language === "" ? undefined : this.#embeds(language)) ?? this.#embeds("@code");
  }

  #syncEmbeds(block: HTMLElement): void {
    for (const element of this.#placeholders(block)) {
      const wanted = this.#lookup(element);
      const current = this.#mounted.get(element);
      if (current?.loader === wanted) continue;
      if (current) this.#unmountOne(element);
      if (wanted) this.#mountOne(element, wanted);
    }
  }

  #mountOne(element: HTMLElement, loader: LazyEmbed): void {
    const isMath = element.hasAttribute("data-math-style");
    const source = isMath
      ? (element.textContent ?? "")
      : (element.querySelector("pre code")?.textContent ?? element.textContent ?? "");
    const record: Mounted = {
      loader,
      original: Array.from(element.childNodes),
      className: element.className,
      instance: null,
      cancelled: false,
    };
    this.#mounted.set(element, record);
    const props: EmbedProps = {
      source,
      info: isMath ? "" : (element.getAttribute("data-info") ?? ""),
      display: element.getAttribute("data-math-style") === "display",
    };
    void loader().then(
      (module) => {
        if (record.cancelled) return;
        // the original content leaves in the same turn the embed arrives: no flash
        element.replaceChildren();
        record.instance = mount(module.default, { target: element, props });
      },
      () => {
        // the library did not load: the plain text stays
      },
    );
  }

  #unmountOne(element: HTMLElement): void {
    const record = this.#mounted.get(element);
    if (!record) return;
    record.cancelled = true;
    this.#mounted.delete(element);
    if (record.instance) {
      void unmount(record.instance);
      element.replaceChildren(...record.original);
      element.className = record.className;
    }
  }

  // ---- links -------------------------------------------------------------------------------------------------

  #decorateLinks(block: HTMLElement): void {
    const anchors = Array.from(block.querySelectorAll<HTMLAnchorElement>("a[href]")).filter(
      (a) => isLocalHref(a.getAttribute("href")) && !a.classList.contains("anchor"),
    );
    if (anchors.length === 0) return;
    const hrefs = anchors.map((a) => a.getAttribute("href") ?? "");
    void this.links.resolve(hrefs).then((targets) => {
      anchors.forEach((a, at) => {
        const target = targets[at];
        if (target?.type !== "missing") return;
        a.classList.add("is-missing");
        a.title = t("link.missing", { path: target.path });
      });
    });
  }

  // ---- code references ---------------------------------------------------------------------------------------

  async #decorateCodeRefs(block: HTMLElement): Promise<void> {
    const doc = this.#doc;
    if (doc === null) return;
    const inline: { element: HTMLElement; ref: CodeRef }[] = [];
    for (const code of block.querySelectorAll<HTMLElement>("code")) {
      if (code.closest("pre, a, .code-block")) continue;
      const ref = parseInlineRef(code.textContent ?? "");
      if (ref) inline.push({ element: code, ref });
    }
    const text: { node: Text; start: number; end: number; ref: CodeRef }[] = [];
    const walker = document.createTreeWalker(
      block,
      NodeFilter.SHOW_ELEMENT | NodeFilter.SHOW_TEXT,
      {
        acceptNode(node) {
          if (node.nodeType === Node.TEXT_NODE) return NodeFilter.FILTER_ACCEPT;
          const tag = node instanceof Element ? node.localName : "";
          return tag === "a" ||
            tag === "code" ||
            tag === "pre" ||
            tag === "script" ||
            tag === "style"
            ? NodeFilter.FILTER_REJECT
            : NodeFilter.FILTER_SKIP;
        },
      },
    );
    for (let node = walker.nextNode(); node !== null; node = walker.nextNode()) {
      if (!(node instanceof Text)) continue;
      for (const found of findTextRefs(node.data)) text.push({ node, ...found });
    }
    if (inline.length === 0 && text.length === 0) return;
    const written = [
      ...new Set([...inline.map((i) => i.ref.path), ...text.map((i) => i.ref.path)]),
    ];
    const result = await this.#api.resolveCodeRefs(doc, written);
    if (!result.ok || doc !== this.#doc) return;
    const files = new Map(written.map((path, at) => [path, result.value[at] ?? []]));
    for (const { element, ref } of inline) {
      const paths = files.get(ref.path) ?? [];
      if (paths.length > 0 && element.isConnected) markCodeRef(element, ref, paths);
    }
    // from the end of each text node to its start, so earlier offsets stay valid
    for (const { node, start, end, ref } of text.reverse()) {
      const paths = files.get(ref.path) ?? [];
      if (paths.length === 0 || !node.isConnected) continue;
      const range = document.createRange();
      range.setStart(node, start);
      range.setEnd(node, end);
      const span = document.createElement("span");
      range.surroundContents(span);
      markCodeRef(span, ref, paths);
    }
  }
}

/** Makes an element a code reference: where it leads and a title that says so. */
function markCodeRef(element: HTMLElement, ref: CodeRef, paths: readonly string[]): void {
  element.classList.add(CODE_REF);
  element.setAttribute("data-ref-paths", paths.join("\n"));
  if (ref.line !== null) element.setAttribute("data-ref-line", String(ref.line));
  if (ref.col !== null) element.setAttribute("data-ref-col", String(ref.col));
  element.tabIndex = 0;
  element.setAttribute("role", "link");
  const first = paths[0] ?? "";
  element.title =
    paths.length > 1
      ? t("codeRef.titleMany", { count: paths.length })
      : ref.line === null
        ? t("codeRef.title", { path: first })
        : t("codeRef.titleAt", { path: refLabel(first, null), line: ref.line });
}

/** What a click on a code reference asks for. */
export type CodeRefTarget = { paths: string[]; line: number | null; col: number | null };

export function codeRefOf(element: Element): CodeRefTarget | null {
  const holder = element.closest<HTMLElement>(`.${CODE_REF}`);
  const paths = holder?.getAttribute("data-ref-paths");
  if (!holder || !paths) return null;
  const number = (name: string): number | null => {
    const value = Number(holder.getAttribute(name));
    return Number.isFinite(value) && value >= 1 ? value : null;
  };
  return { paths: paths.split("\n"), line: number("data-ref-line"), col: number("data-ref-col") };
}
