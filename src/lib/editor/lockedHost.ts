// How the locked blocks of Visual mode are drawn: with the reading view's own HTML for the block, so a diagram is a diagram and
// a table of raw HTML looks like it does when reading. The editor core only knows the `LockedHost` interface; this is the app's
// side of it. HTML comes from the backend in batches (one call for all the blocks that appeared in the same moment).

import { api } from "../api";
import {
  enhanceBlocks,
  renderDiagram,
  stashRelativeImages,
  type EnhanceContext,
} from "../dom/enhance";
import type { BlockKind } from "../types";
import type { LockedHost, LockReason } from "./visual/core";

type Atom = { kind: BlockKind; source: string; reason: LockReason; definitions?: string };
type Entry = { body: HTMLElement; atom: Atom };

export type LockedHostDeps = {
  /** The context the reading view enhances its HTML with (images relative to the document, diagrams in the theme, labels). */
  context(): EnhanceContext;
  /** The "Edit in Source" button of a block. */
  edit(block: number | null, source: string): void;
  /** A click on a link inside a block (already kept from navigating the window): follow it as the reading view does. */
  link(href: string): void;
  labels: LockedHost["labels"];
};

export type LockedHostHandle = {
  host: LockedHost;
  /** The theme changed: the diagrams are drawn again. */
  retheme(): void;
  dispose(): void;
};

function sourceView(body: HTMLElement, source: string): void {
  const pre = document.createElement("pre");
  pre.className = "tm-locked__source";
  pre.textContent = source.replace(/\r\n|\r/g, "\n");
  body.replaceChildren(pre);
}

export function createLockedHost(deps: LockedHostDeps): LockedHostHandle {
  /** Block source to its HTML ("" when the backend has none for it). */
  const cache = new Map<string, string>();
  const waiting = new Map<string, Entry[]>();
  const live = new Set<Entry>();
  let scheduled = false;
  let disposed = false;

  function fill(entry: Entry, html: string): void {
    if (html === "") {
      sourceView(entry.body, entry.atom.source);
      return;
    }
    const template = document.createElement("template");
    template.innerHTML = html;
    const context = deps.context();
    stashRelativeImages(template.content, context.docPath);
    const elements = Array.from(template.content.children) as HTMLElement[];
    entry.body.replaceChildren(template.content);
    enhanceBlocks(elements, context);
  }

  function flush(): void {
    scheduled = false;
    const sources = [...waiting.keys()];
    if (sources.length === 0 || disposed) return;
    const entries = new Map(waiting);
    waiting.clear();
    void api
      .renderFragments(sources)
      .then((rendered) => {
        sources.forEach((source, i) => {
          const html = rendered[i] ?? "";
          cache.set(source, html);
          if (disposed) return;
          for (const entry of entries.get(source) ?? []) fill(entry, html);
        });
      })
      .catch(() => {
        // the source stays on screen; nothing is lost
      });
  }

  const host: LockedHost = {
    render(body, atom) {
      const entry: Entry = { body, atom };
      live.add(entry);
      if (atom.kind === "frontmatter") {
        sourceView(body, atom.source);
        return;
      }
      // a block that uses a reference is sent together with the definitions it uses, so that its links and footnote references
      // resolve as they do in the reading view. The definitions draw nothing of their own: a link definition is not shown, and a
      // footnote is a block of its own (the backend draws the reference as a marker and leaves the footnote out of the block)
      const markdown = atom.definitions ? `${atom.source}\n\n${atom.definitions}` : atom.source;
      const cached = cache.get(markdown);
      if (cached !== undefined) {
        fill(entry, cached);
        return;
      }
      sourceView(body, atom.source);
      const list = waiting.get(markdown) ?? [];
      list.push(entry);
      waiting.set(markdown, list);
      if (!scheduled) {
        scheduled = true;
        queueMicrotask(flush);
      }
    },
    edit: (block, source) => deps.edit(block, source),
    link: (href) => deps.link(href),
    labels: deps.labels,
  };

  return {
    host,
    retheme() {
      const context = deps.context();
      for (const entry of [...live]) {
        if (!entry.body.isConnected) {
          live.delete(entry);
          continue;
        }
        entry.body.querySelectorAll<HTMLElement>("figure.diagram").forEach((figure) => {
          void renderDiagram(figure, context);
        });
      }
    },
    dispose() {
      disposed = true;
      cache.clear();
      waiting.clear();
      live.clear();
    },
  };
}
