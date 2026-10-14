// Renders a DocumentView into the document body and, on a live update, diffs it against the previous
// render: changed/added blocks get `.tm-changed`, removed runs get a `.tm-removed` marker, and the scroll
// position stays anchored to a block that exists in both versions.

import { diffBlocks, pickAnchor, type BlockDiff, type RemovedRun } from "../diff";
import { hashString } from "../text";
import type { DocumentView } from "../types";
import { enhanceBlocks, renderDiagram, stashRelativeImages, type EnhanceContext } from "./enhance";

export type RenderLabels = {
  /** "1 block removed · 10:42" */
  removed: (count: number) => string;
  show: string;
  hide: string;
  showTitle: string;
};

export type RenderOptions = {
  /** "update": the same document was already on screen (diff + scroll anchoring). */
  mode: "fresh" | "update";
  /** A fresh open that should still mark what changed since the document was last rendered. */
  diffAgainstLast?: boolean;
  /** An update that is not news (the person's own edit): the new version is shown without marks. */
  quiet?: boolean;
  enhance: EnhanceContext;
  labels: RenderLabels;
};

export type RenderResult = {
  changed: HTMLElement[];
  removedMarkers: HTMLElement[];
  /** Resolves when the lazy renderers (highlight.js, KaTeX, Mermaid, images) have finished. */
  settled: Promise<void>;
};

type Snapshot = {
  keys: string[];
  /** "backend": keys are `DocumentView.blocks`; "dom": hashed from the rendered elements. */
  source: "backend" | "dom";
  /** The original HTML of every block, used to show removed blocks. */
  pristine: string[];
};

type Candidate = { index: number; offset: number };

const MAX_CANDIDATES = 24;

function domKey(el: Element): string {
  return hashString(el.outerHTML.replace(/ data-sourcepos="[^"]*"/g, ""));
}

/** HTML of an old block that is safe to show again: no ids (duplicates) and no image requests. */
function inertHtml(html: string): string {
  return html.replace(/ id="[^"]*"/g, "").replace(/ src="/g, ' data-src="');
}

export class DocRenderer {
  /** What the reader last saw of each document, so a doc opened because it changed can be marked. */
  private snapshots = new Map<string, Snapshot>();
  private blockEls: HTMLElement[] = [];

  constructor(
    private readonly body: HTMLElement,
    private readonly scroller: HTMLElement,
  ) {}

  /** The top-level element of every block, in document order. */
  get blocks(): readonly HTMLElement[] {
    return this.blockEls;
  }

  /** Drops the rendered blocks (a different document is about to be shown). */
  reset(): void {
    this.blockEls = [];
  }

  clear(): void {
    this.reset();
    this.body.replaceChildren();
  }

  /** Forgets everything seen so far (another project was opened). */
  forget(): void {
    this.snapshots.clear();
    this.reset();
  }

  render(view: DocumentView, opts: RenderOptions): RenderResult {
    const template = document.createElement("template");
    template.innerHTML = view.html;
    const fragment = template.content;
    stashRelativeImages(fragment, view.path);

    const els = Array.from(fragment.children) as HTMLElement[];
    const pristine = els.map((el) => el.outerHTML);
    const source: Snapshot["source"] = els.length === view.blocks.length ? "backend" : "dom";
    const keys = source === "backend" ? view.blocks : els.map(domKey);

    const prev = this.snapshots.get(view.path) ?? null;
    const wantsDiff = (opts.mode === "update" && !opts.quiet) || opts.diffAgainstLast === true;
    const diff: BlockDiff | null =
      wantsDiff && prev !== null && prev.source === source ? diffBlocks(prev.keys, keys) : null;
    const anchoring = diff !== null && opts.mode === "update";
    const candidates = anchoring ? this.visibleCandidates() : [];
    const scrollTop = this.scroller.scrollTop;

    this.body.replaceChildren(fragment);
    const enhanced = enhanceBlocks(els, opts.enhance);
    this.blockEls = enhanced.els;

    const changed: HTMLElement[] = [];
    const removedMarkers: HTMLElement[] = [];
    if (diff && prev) {
      for (const index of diff.added) {
        const el = this.blockEls[index];
        if (!el) continue;
        el.classList.add("tm-changed");
        changed.push(el);
      }
      for (const run of diff.removed) {
        const marker = this.removedMarker(run, prev.pristine, opts.labels);
        const before = this.blockEls[run.beforeNew] ?? null;
        this.body.insertBefore(marker, before);
        removedMarkers.push(marker);
      }
      if (anchoring) this.restoreAnchor(diff, candidates, scrollTop);
    } else if (opts.mode === "update") {
      this.scroller.scrollTo({ top: scrollTop, behavior: "instant" });
    }

    this.snapshots.delete(view.path);
    this.snapshots.set(view.path, { keys, source, pristine });
    if (this.snapshots.size > 24) {
      const oldest = this.snapshots.keys().next().value;
      if (oldest !== undefined) this.snapshots.delete(oldest);
    }
    const settled = Promise.allSettled(enhanced.tasks).then(() => undefined);
    return { changed, removedMarkers, settled };
  }

  /** Re-renders the Mermaid diagrams after a theme change. */
  async rethemeDiagrams(ctx: EnhanceContext): Promise<void> {
    const figures = Array.from(
      this.body.querySelectorAll<HTMLElement>(".diagram[data-source], .diagram-error[data-source]"),
    );
    await Promise.allSettled(figures.map((figure) => renderDiagram(figure, ctx)));
  }

  // ---- scroll anchoring -------------------------------------------------------------------------

  private visibleCandidates(): Candidate[] {
    const top = this.scroller.getBoundingClientRect().top;
    const out: Candidate[] = [];
    for (let index = 0; index < this.blockEls.length; index++) {
      const rect = this.blockEls[index].getBoundingClientRect();
      if (rect.bottom <= top + 1) continue;
      out.push({ index, offset: rect.top - top });
      if (out.length >= MAX_CANDIDATES) break;
    }
    return out;
  }

  private restoreAnchor(diff: BlockDiff, candidates: Candidate[], previousTop: number): void {
    const anchor = pickAnchor(
      candidates.map((c) => c.index),
      diff,
    );
    const target = anchor ? this.blockEls[anchor.newIndex] : undefined;
    const before = candidates.find((c) => c.index === anchor?.oldIndex);
    if (!anchor || !target || !before) {
      this.scroller.scrollTo({ top: previousTop, behavior: "instant" });
      return;
    }
    const top = this.scroller.getBoundingClientRect().top;
    const delta = target.getBoundingClientRect().top - top - before.offset;
    if (Math.abs(delta) > 0.5) {
      this.scroller.scrollTo({ top: this.scroller.scrollTop + delta, behavior: "instant" });
    }
  }

  // ---- removed marker ---------------------------------------------------------------------------

  private removedMarker(run: RemovedRun, pristine: string[], labels: RenderLabels): HTMLElement {
    const marker = document.createElement("div");
    marker.className = "tm-removed";

    const bar = document.createElement("button");
    bar.type = "button";
    bar.className = "tm-removed__bar";
    bar.setAttribute("aria-expanded", "false");
    bar.title = labels.showTitle;
    const rule1 = document.createElement("span");
    rule1.className = "tm-removed__rule";
    const label = document.createElement("span");
    label.className = "tm-removed__label";
    label.textContent = labels.removed(run.oldIndices.length);
    const rule2 = document.createElement("span");
    rule2.className = "tm-removed__rule";
    const toggle = document.createElement("span");
    toggle.className = "tm-removed__toggle";
    toggle.textContent = labels.show;
    bar.append(rule1, label, rule2, toggle);

    const content = document.createElement("div");
    content.className = "tm-removed__content";
    content.hidden = true;
    content.innerHTML = run.oldIndices.map((i) => inertHtml(pristine[i] ?? "")).join("\n");

    bar.addEventListener("click", () => {
      const open = bar.getAttribute("aria-expanded") !== "true";
      bar.setAttribute("aria-expanded", String(open));
      content.hidden = !open;
      toggle.textContent = open ? labels.hide : labels.show;
    });
    marker.append(bar, content);
    return marker;
  }
}
