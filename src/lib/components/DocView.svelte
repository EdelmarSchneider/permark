<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { bridge } from "../bridge";
  import { clearCodeRefs, decorateCodeRefs, refTargetOf, type RefTarget } from "../dom/codeRefsDom";
  import { codeRefsContextFor } from "../dom/codeRefsContext";
  import { DomFind } from "../dom/domFind";
  import { DocRenderer, type RenderLabels, type RenderResult } from "../dom/docRender";
  import { anchorTarget, findById, selectionIn, selectionLines } from "../dom/domUtil";
  import { commentTargetIn } from "../dom/commentDom";
  import { readBlocks, readHeadingLines } from "../dom/outlineDom";
  import { enhanceContextFor } from "../dom/enhanceContext";
  import type { EnhanceContext } from "../dom/enhance";
  import { ICON_CHECK, ICON_COPY } from "../dom/enhance";
  import { showsAtTop } from "../editor/viewport";
  import { isMediaViewKind } from "../fileKinds";
  import { frontmatterValue } from "../frontmatter";
  import { t, tn } from "../i18n";
  import { barFor, type BlockBox } from "../commentBars";
  import { barComments } from "../comments";
  import { buildOutline, currentHeading } from "../outline";
  import { assetUrl } from "../platform";
  import { basename, decodeFragment } from "../paths";
  import { blockForLine, parseSourcepos } from "../sourcepos";
  import { app } from "../stores/app.svelte";
  import { docs, type LocateRequest, type RenderRequest } from "../stores/docs.svelte";
  import { comments } from "../stores/comments.svelte";
  import { outline } from "../stores/outline.svelte";
  import { session } from "../stores/session.svelte";
  import { tone } from "../stores/toneChoice.svelte";
  import { toast } from "../stores/toast.svelte";
  import { ui } from "../stores/ui.svelte";
  import { unread } from "../stores/unread.svelte";
  import { vault } from "../stores/vault.svelte";
  import { clock as formatClock } from "../time";
  import type { DocumentView } from "../types";
  import CodeRefMenu, { type RefMenu } from "./CodeRefMenu.svelte";
  import Icon from "./Icon.svelte";
  import Notice from "./Notice.svelte";
  import DocMetaStrip from "./DocMetaStrip.svelte";
  import MediaView from "./MediaView.svelte";
  import Popover from "./Popover.svelte";

  let scroller: HTMLElement | undefined = $state();
  let body: HTMLElement | undefined = $state();
  let renderer: DocRenderer | null = null;
  let finder: DomFind | null = null;
  let renderedRev = 0;
  let handledJump = 0;
  /** True once the reader scrolled by hand since the last render, so late layout changes must not move them. */
  let userScrolled = false;
  let locatedClearTimer: ReturnType<typeof setTimeout> | null = null;
  /** The blocks wearing the search-hit tint right now. */
  let tinted: HTMLElement[] = [];
  /** The heading elements of the outline, in order (the outline holds only their text). */
  let headingEls: HTMLElement[] = [];
  /** The heading an outline click went to, kept as the section being read until the reader scrolls by hand. */
  let pinned: number | null = null;
  let frame = 0;
  let missing = $state<{ anchor: DOMRect; path: string } | null>(null);
  let refMenu = $state<RefMenu | null>(null);
  let article: HTMLElement | undefined = $state();
  /** A bar in the margin beside the passage of an open comment. */
  type CommentBar = { id: string; top: number; height: number; text: string };
  let commentBars = $state.raw<CommentBar[]>([]);
  let barFrame = 0;

  const view = $derived(docs.current);
  /** A picture, a sound or a video: drawn from the file itself by the media view, with no text to render. */
  const mediaView = $derived(view && isMediaViewKind(view.kind) ? { ...view, kind: view.kind } : null);
  const showArticle = $derived(
    !!view && view.kind !== "binary" && view.kind !== "tooLarge" && !mediaView,
  );
  const tombstone = $derived(docs.deleted !== null);
  // `tags:` moved to the strip above the document
  const frontmatter = $derived(
    (view?.frontmatter ?? []).filter(([key]) => key.toLowerCase() !== "tags"),
  );

  // ---- rendering ----------------------------------------------------------------------------------

  // The code references (`src/main.rs:42`) are links only here, and only with a project to look in (Simple mode has none).
  const enhanceContext = (path: string): EnhanceContext => ({
    ...enhanceContextFor(path, session.resolvedTheme),
    codeRefs: vault.isSimple ? undefined : codeRefsContextFor(vault.files, path),
  });

  function renderLabels(): RenderLabels {
    const time = formatClock(Date.now());
    return {
      removed: (count) => tn("doc.removedBlocks", count, { time }),
      show: t("doc.show"),
      hide: t("doc.hide"),
      showTitle: t("doc.showRemoved"),
    };
  }

  function ensureObjects(): { renderer: DocRenderer; finder: DomFind } | null {
    if (!body || !scroller) return null;
    renderer ??= new DocRenderer(body, scroller);
    finder ??= new DomFind(
      () => body ?? null,
      () => scroller ?? null,
      (state) => {
        ui.find.total = state.total;
        ui.find.index = state.index;
      },
    );
    return { renderer, finder };
  }

  function apply(req: RenderRequest): void {
    if (req.rev === renderedRev) return;
    renderedRev = req.rev;
    const objects = ensureObjects();
    if (!objects) return;
    const { renderer, finder } = objects;
    missing = null;
    const view = req.view;

    if (view.kind === "binary" || view.kind === "tooLarge" || isMediaViewKind(view.kind)) {
      renderer.clear();
      finder.clear();
      clearOutline();
      docs.reportRender({ changed: 0, removed: 0 });
      scroller?.scrollTo({ top: 0, behavior: "instant" });
      return;
    }
    if (req.mode === "fresh") {
      renderer.reset();
      userScrolled = false;
      lastChange = null;
    }

    const result = renderer.render(view, {
      mode: req.mode,
      diffAgainstLast: req.diffAgainstLast,
      quiet: req.quiet,
      enhance: enhanceContext(view.path),
      labels: renderLabels(),
    });
    docs.reportRender({ changed: result.changed.length, removed: result.removedMarkers.length });
    reportOutline(view);
    scheduleBars();

    if (req.mode === "fresh") {
      position(req, result);
      void result.settled.then(() => {
        // late layout changes (diagrams, math, images) move content: put the reader back, unless they scrolled
        if (renderedRev === req.rev && !userScrolled) position(req, result, true);
      });
    } else if (req.followScroll && result.changed.length + result.removedMarkers.length > 0) {
      jumpToChange(1, true);
    }

    if (req.locate?.find) {
      const find = req.locate.find;
      ui.find.query = find.query;
      ui.find.mode = find.mode;
      ui.find.open = true;
      ui.find.takeFocus = false;
    }
    // diagrams, math and images change the layout once they have loaded
    void result.settled.then(() => {
      if (renderedRev !== req.rev) return;
      updateCurrent();
      scheduleBars();
    });
    const rerun = (keep: boolean): void => {
      if (renderedRev !== req.rev) return;
      if (ui.find.open && ui.find.query.trim() !== "") {
        finder.run(ui.find.query, ui.find.mode, {
          keepIndex: keep,
          near: keep ? null : locatedElement,
        });
      }
    };
    rerun(req.mode === "update");
    void result.settled.then(() => rerun(true));
  }

  // ---- positioning -------------------------------------------------------------------------------

  let locatedElement: HTMLElement | null = null;
  /** The changed block F7 jumped to last. */
  let lastChange: HTMLElement | null = null;

  function scrollToElement(el: HTMLElement, ratio: number, margin = 0): void {
    if (!scroller) return;
    const delta = el.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
    const top = Math.max(0, scroller.scrollTop + delta - ratio * scroller.clientHeight - margin);
    scroller.scrollTo({ top, behavior: "instant" });
  }

  /** Puts a freshly opened document where it belongs; `again` repeats it quietly once the layout settled. */
  function position(req: RenderRequest, result: RenderResult, again = false): void {
    if (!scroller) return;
    if (req.locate && locate(req.locate, !again)) return;
    if (req.anchor && scrollToAnchor(req.anchor, false, !again)) return;
    if (req.scrollTop !== null) {
      scroller.scrollTo({ top: req.scrollTop, behavior: "instant" });
      return;
    }
    if (req.followScroll && result.changed.length + result.removedMarkers.length > 0) {
      jumpToChange(1, true);
      return;
    }
    scroller.scrollTo({ top: 0, behavior: "instant" });
  }

  /** Scrolls to the block that covers a search hit's line and tints it briefly. */
  function locate(request: LocateRequest, tint = true): boolean {
    const blocks = renderer?.blocks ?? [];
    const ranges = blocks.map((el) => parseSourcepos(el.getAttribute("data-sourcepos")));
    const index = blockForLine(ranges, request.line);
    if (index === null) return false;
    const target = blocks[index];
    if (request.align === "top") scrollToElement(target, 0, 8);
    else scrollToElement(target, 1 / 3);
    locatedElement = target;
    if (!tint || request.tint === false) return true;
    const next = blocks[index + 1];
    tintBlocks(next && !/^H[1-6]$/.test(next.tagName) ? [target, next] : [target]);
    return true;
  }

  /** Tints blocks for a moment, like a search hit (the heading of a clicked outline entry uses it too). */
  function tintBlocks(elements: HTMLElement[]): void {
    for (const el of tinted) el.classList.remove("tm-located");
    tinted = elements;
    for (const el of elements) {
      void el.offsetWidth; // restart the animation when the same block is tinted again
      el.classList.add("tm-located");
    }
    if (locatedClearTimer !== null) clearTimeout(locatedClearTimer);
    locatedClearTimer = setTimeout(() => {
      for (const el of elements) el.classList.remove("tm-located");
    }, 1700);
  }

  function scrollToAnchor(id: string, smooth = true, notify = true): boolean {
    if (!body || !scroller) return false;
    const found = findById(body, id);
    if (!found) {
      if (notify) toast.message(t("doc.sectionMissing", { id: decodeFragment(id) }), "warn");
      return false;
    }
    const target = anchorTarget(found);
    if (smooth) {
      const delta = target.getBoundingClientRect().top - scroller.getBoundingClientRect().top;
      scroller.scrollTo({ top: Math.max(0, scroller.scrollTop + delta - 16), behavior: "smooth" });
    } else {
      scrollToElement(target, 0, 16);
    }
    return true;
  }

  /** Next or previous changed block (or removed marker); `landing` puts it near the top. */
  function jumpToChange(direction: 1 | -1, first = false): void {
    if (!body || !scroller) return;
    const marks = Array.from(body.querySelectorAll<HTMLElement>(".tm-changed, .tm-removed"));
    const starts = marks.filter((m) => {
      const prev = m.previousElementSibling;
      return !(prev instanceof HTMLElement && marks.includes(prev));
    });
    if (starts.length === 0) return;
    const box = scroller.getBoundingClientRect();
    const landing = 0.2 * scroller.clientHeight;
    const offsetOf = (el: HTMLElement): number => el.getBoundingClientRect().top - box.top;
    let target: HTMLElement | undefined;
    const lastIndex = lastChange ? starts.indexOf(lastChange) : -1;
    const lastRect = lastChange?.getBoundingClientRect();
    const lastVisible = !!lastRect && lastRect.bottom > box.top && lastRect.top < box.bottom;
    if (first) target = starts[0];
    else if (lastIndex >= 0 && lastVisible) {
      // continue from the block we jumped to last (it may sit lower than the landing line at the end of the page)
      target = starts[(lastIndex + direction + starts.length) % starts.length];
    } else if (direction === 1) {
      target = starts.find((el) => offsetOf(el) > landing + 8) ?? starts[0];
    } else {
      target =
        [...starts].reverse().find((el) => offsetOf(el) < landing - 8) ?? starts[starts.length - 1];
    }
    lastChange = target;
    const top = Math.max(0, scroller.scrollTop + offsetOf(target) - landing);
    scroller.scrollTo({ top, behavior: first ? "instant" : "smooth" });
  }

  // ---- outline ---------------------------------------------------------------------------------------

  function clearOutline(): void {
    headingEls = [];
    pinned = null;
    outline.clear();
  }

  /**
   * Rebuilds the outline from the headings on screen (after every render, fresh or live). While editing, this view is hidden and the
   * editor builds the outline (`editor/outlineSync.ts`).
   */
  function reportOutline(view: DocumentView): void {
    if (docs.editing) return;
    if (!body || view.kind !== "markdown") {
      clearOutline();
      return;
    }
    const { blocks, headings } = readBlocks(body);
    const model = buildOutline(blocks);
    headingEls = headings.slice(0, model.items.length);
    pinned = null;
    outline.set(view.path, model);
    updateCurrent();
  }

  /** The section being read: the last heading above the top-third line of the viewport. */
  function updateCurrent(): void {
    if (docs.editing) return;
    if (!scroller || headingEls.length === 0) {
      outline.setCurrent(null);
      return;
    }
    if (pinned !== null) {
      outline.setCurrent(pinned);
      return;
    }
    const top = scroller.getBoundingClientRect().top;
    const height = scroller.clientHeight;
    const atBottom = scroller.scrollTop + height >= scroller.scrollHeight - 2;
    outline.setCurrent(
      currentHeading(headingEls.length, (i) => headingEls[i].getBoundingClientRect().top - top, {
        line: height / 3,
        bottomEdge: atBottom ? height : undefined,
      }),
    );
  }

  /** An outline click: the heading goes to the top (8px below it) and is tinted, with no history entry. */
  function goToHeading(index: number): void {
    const el = headingEls[index];
    if (!el) return;
    userScrolled = true; // late layout changes must not pull the page back
    scrollToElement(el, 0, 8);
    tintBlocks([el]);
    // near the end of the page the heading cannot reach the top, so it stays the current one until the reader scrolls
    pinned = index;
    outline.setCurrent(index);
  }

  // ---- comments: the bars in the margin ----------------------------------------------------------

  /** The blocks of the rendered document (the ones that know their lines), measured from `origin`. */
  function measureBlocks(origin: number): BlockBox[] {
    const boxes: BlockBox[] = [];
    for (const el of body?.querySelectorAll<HTMLElement>("[data-sourcepos]") ?? []) {
      const lines = parseSourcepos(el.getAttribute("data-sourcepos"));
      const rect = el.getBoundingClientRect();
      if (!lines || rect.height === 0) continue;
      // a list item that holds a list ends where the list begins: its own text is what a comment is on
      const nested = el.tagName === "LI" ? el.querySelector(":scope > ul, :scope > ol") : null;
      let depth = 0;
      for (let up = el.parentElement; up && up !== body; up = up.parentElement) depth++;
      boxes.push({
        ...lines,
        depth,
        top: rect.top - origin,
        bottom: (nested ? nested.getBoundingClientRect().top : rect.bottom) - origin,
      });
    }
    return boxes;
  }

  /** Measures the bars of the open comments of the document on show. */
  function layoutBars(): void {
    barFrame = 0;
    const path = docs.current?.path ?? null;
    const list = body && article && !docs.editing ? barComments(comments.all, path) : [];
    if (list.length === 0) {
      if (commentBars.length > 0) commentBars = [];
      return;
    }
    const boxes = measureBlocks(article?.getBoundingClientRect().top ?? 0);
    commentBars = list.flatMap((comment) => {
      const bar = barFor(boxes, comment.line, comment.endLine);
      return bar ? [{ id: comment.id, ...bar, text: comment.text }] : [];
    });
  }

  /** Measures the bars once the page has settled (a burst of changes asks once). */
  function scheduleBars(): void {
    if (barFrame === 0) barFrame = requestAnimationFrame(layoutBars);
  }

  // ---- effects ----------------------------------------------------------------------------------------

  // the comments, the document or the way it is shown changed: the bars are measured again
  $effect(() => {
    void comments.all;
    void docs.current?.path;
    void docs.editing;
    untrack(scheduleBars);
  });

  $effect(() => {
    const req = docs.request;
    if (!req || !body || !scroller) return;
    untrack(() => apply(req));
  });

  $effect(() => {
    const jump = docs.jump;
    if (!jump || jump.n === handledJump || !renderer) return;
    untrack(() => {
      handledJump = jump.n;
      if (jump.locate) {
        if (locate(jump.locate) && jump.locate.find) {
          ui.find.query = jump.locate.find.query;
          ui.find.mode = jump.locate.find.mode;
          ui.find.open = true;
          ui.find.takeFocus = false;
          finder?.run(ui.find.query, ui.find.mode, { near: locatedElement });
        }
      } else if (jump.anchor) {
        scrollToAnchor(jump.anchor);
      }
    });
  });

  // the diagrams are drawn in the colours of the theme, and of the tone of the light theme
  let lastTheme = untrack(() => `${session.resolvedTheme}:${tone.current}`);
  $effect(() => {
    const theme = `${session.resolvedTheme}:${tone.current}`;
    untrack(() => {
      if (theme === lastTheme) return;
      lastTheme = theme;
      if (view && renderer) void renderer.rethemeDiagrams(enhanceContext(view.path));
    });
  });

  /** Looks the code references up again, in the text that is on screen. */
  function refreshCodeRefs(): void {
    if (!body || !view || view.kind === "binary" || view.kind === "tooLarge" || mediaView) return;
    clearCodeRefs(body);
    if (!vault.isSimple) decorateCodeRefs(body, codeRefsContextFor(vault.files, view.path));
    // the find highlights point into text that was just cut up
    if (ui.find.open && ui.find.query.trim() !== "") {
      finder?.run(ui.find.query, ui.find.mode, { keepIndex: true });
    }
  }

  // The project's files changed (one was made, moved or removed): a reference may have become a file, or stopped being one.
  let lastFiles = untrack(() => vault.files);
  $effect(() => {
    const files = vault.files;
    untrack(() => {
      if (files === lastFiles) return;
      lastFiles = files;
      refreshCodeRefs();
    });
  });

  // Editing hides the reading view; a hidden scroller forgets where it was, so the position is taken first and put back.
  let scrollBeforeEdit = 0;
  $effect.pre(() => {
    if (docs.editing && scroller) scrollBeforeEdit = untrack(() => scroller?.scrollTop ?? 0);
  });
  $effect(() => {
    if (docs.editing || !scroller) return;
    const top = scrollBeforeEdit;
    scrollBeforeEdit = 0;
    if (top > 0) untrack(() => scroller?.scrollTo({ top, behavior: "instant" }));
  });

  // Back from editing: the outline of the editor gives way to the one of the reading view.
  let wasEditing = false;
  $effect(() => {
    const editing = docs.editing;
    untrack(() => {
      if (wasEditing && !editing && view) reportOutline(view);
      wasEditing = editing;
    });
  });

  // no document (closed, or the project was replaced): no outline
  $effect(() => {
    if (!docs.current) untrack(clearOutline);
  });

  onMount(() => {
    bridge.jumpChange = (direction) => jumpToChange(direction);
    // while editing, the outline's clicks and focus belong to the editor
    bridge.outline = {
      goTo: (index) => (bridge.editorOutline ?? { goTo: goToHeading }).goTo(index),
      focusDoc: () =>
        bridge.editorOutline
          ? bridge.editorOutline.focusDoc()
          : scroller?.focus({ preventScroll: true }),
    };
    bridge.find = {
      run: (query, mode, opts) => ensureObjects()?.finder.run(query, mode, opts),
      step: (direction) => finder?.step(direction),
      clear: () => finder?.clear(),
    };
    bridge.getSelection = () => {
      if (!body) return null;
      const selection = selectionIn(body);
      return selection ? { text: selection.text, section: selection.section } : null;
    };
    bridge.locatedElement = () => locatedElement;
    bridge.topLine = () => {
      if (!scroller) return null;
      const edge = scroller.getBoundingClientRect().top;
      for (const el of renderer?.blocks ?? []) {
        const box = el.getBoundingClientRect();
        if (!showsAtTop(box.top, box.bottom, edge)) continue;
        return parseSourcepos(el.getAttribute("data-sourcepos"))?.startLine ?? null;
      }
      return null;
    };
    bridge.readingSnapshot = () => {
      if (!body) return null;
      const { headings, lastLine } = readHeadingLines(body);
      const selected = selectionIn(body);
      return {
        headings,
        lastLine,
        selection: selected ? { text: selected.text, ...selectionLines(body) } : null,
      };
    };
    bridge.commentTarget = () => (body ? commentTargetIn(body) : null);
    // images, diagrams, the zoom and the reading width change the height of what is above a comment
    const watcher = typeof ResizeObserver === "undefined" ? null : new ResizeObserver(scheduleBars);
    if (body) watcher?.observe(body);
    return () => {
      watcher?.disconnect();
      bridge.commentTarget = undefined;
      if (barFrame) cancelAnimationFrame(barFrame);
      bridge.readingSnapshot = undefined;
      bridge.topLine = undefined;
      bridge.jumpChange = undefined;
      bridge.find = undefined;
      bridge.getSelection = undefined;
      bridge.locatedElement = undefined;
      bridge.outline = undefined;
      if (frame) cancelAnimationFrame(frame);
    };
  });

  // ---- events -----------------------------------------------------------------------------------------

  function onscroll(): void {
    if (frame || !scroller) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (scroller) docs.noteScroll(scroller.scrollTop);
      // scrolling the reader did by hand counts as reading; a scroll the app made (a restored position) does not
      if (userScrolled) unread.scrolled();
      updateCurrent();
    });
  }

  /** The reader took over scrolling (wheel, touch, keys or the scrollbar). */
  function took(): void {
    userScrolled = true;
    pinned = null;
  }

  function onwheel(e: WheelEvent): void {
    took();
    if (!e.ctrlKey) return;
    e.preventDefault();
    session.zoomBy(e.deltaY < 0 ? 1 : -1);
  }

  async function copyCode(button: HTMLElement): Promise<void> {
    const code = button.closest(".code-block")?.querySelector("code")?.textContent ?? "";
    await app.copy(code, "code");
    button.innerHTML = ICON_CHECK;
    button.classList.add("is-done");
    setTimeout(() => {
      button.innerHTML = ICON_COPY;
      button.classList.remove("is-done");
    }, 1200);
  }

  /** A click (or Enter) on a code reference: the file it names opens, or a menu asks which of the files it could be. */
  function followRef(ref: RefTarget, el: Element, external: boolean): void {
    if (ref.path !== null) {
      void app.openCodeRef(ref.path, ref.line, ref.col, external);
      return;
    }
    refMenu = { mode: "choose", target: ref, anchor: el.getBoundingClientRect() };
  }

  function oncontextmenu(e: MouseEvent): void {
    const el = (e.target as Element).closest(".code-ref");
    const ref = refTargetOf(el);
    if (!el || !ref) return;
    e.preventDefault();
    // from the keyboard the menu opens at the reference, not at the corner of the window
    const keyboard = e.clientX === 0 && e.clientY === 0;
    const anchor = keyboard
      ? el.getBoundingClientRect()
      : { left: e.clientX, right: e.clientX, top: e.clientY, bottom: e.clientY };
    refMenu = { mode: ref.path !== null ? "actions" : "choose", target: ref, anchor };
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key !== "Enter") return;
    const el = (e.target as Element).closest(".code-ref");
    const ref = refTargetOf(el);
    if (!el || !ref) return;
    e.preventDefault();
    followRef(ref, el, e.ctrlKey || e.metaKey);
  }

  async function onclick(e: MouseEvent): Promise<void> {
    const target = e.target as Element;
    const copy = target.closest<HTMLElement>(".code-block__copy");
    if (copy) {
      void copyCode(copy);
      return;
    }
    const codeRef = target.closest(".code-ref");
    const ref = refTargetOf(codeRef);
    if (codeRef && ref) {
      e.preventDefault();
      followRef(ref, codeRef, e.ctrlKey || e.metaKey);
      return;
    }
    const link = target.closest<HTMLAnchorElement>("a[href]");
    if (link) {
      e.preventDefault();
      const href = link.getAttribute("href") ?? "";
      if (!view || href === "") return;
      if (href.startsWith("#")) {
        scrollToAnchor(href.slice(1));
        return;
      }
      const result = await app.followLink(view.path, href);
      if (result) {
        link.classList.add("is-missing");
        missing = { anchor: link.getBoundingClientRect(), path: result.missing };
      }
      return;
    }
    const image = target.closest<HTMLImageElement>("img");
    if (image && image.src && !image.classList.contains("is-broken")) ui.zoomImage = image.src;
  }
</script>

<!-- the scroller only listens for the reader scrolling (wheel, touch, keys, scrollbar) -->
<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  bind:this={scroller}
  class={["doc-scroll", mediaView && "doc-scroll--media"]}
  hidden={docs.editing}
  role="region"
  tabindex="-1"
  aria-label={t("doc.region")}
  {onscroll}
  {onwheel}
  ontouchmove={took}
  onkeydown={took}
  onpointerdown={(e) => e.target === e.currentTarget && took()}
>
  <DocMetaStrip />
  {#if view && view.kind === "binary"}
    <Notice
      icon="file"
      title={t("doc.binary.title")}
      text={t("doc.binary.text", { name: basename(view.path) })}
    >
      {@render fileActions(view.path)}
    </Notice>
  {:else if mediaView}
    <MediaView view={mediaView} />
  {:else if view && view.kind === "tooLarge"}
    <Notice
      icon="file"
      title={t("doc.tooLarge.title")}
      text={t("doc.tooLarge.text", { name: basename(view.path) })}
    >
      {@render fileActions(view.path)}
    </Notice>
  {:else if !view && docs.loading}
    <div class="skeleton" aria-hidden="true">
      <span></span><span></span><span></span><span></span>
    </div>
  {:else if !view && docs.error}
    <Notice icon="circle-alert" title={t("doc.error.title")} text={docs.error.message}>
      <button type="button" class="btn" onclick={() => (docs.error = null)}
        >{t("common.dismiss")}</button
      >
    </Notice>
  {:else if !view && !vault.isSimple}
    <Notice icon="files" title={t("doc.none.title")} text={t("doc.none.text")} />
  {/if}

  <div class="doc-column">
    <article
      bind:this={article}
      class={["md", tombstone && "is-tombstone"]}
      style:--zoom={session.zoom}
      hidden={!showArticle}
      aria-busy={docs.loading}
    >
      {#if frontmatter.length > 0}
        <dl class="frontmatter" aria-label={t("doc.frontmatter")}>
          {#each frontmatter as [key, value], i (i)}
            {@const parsed = frontmatterValue(key, value)}
            <div>
              <dt>{key}</dt>
              <dd>
                {#if "chips" in parsed}
                  {#each parsed.chips as chip, c (c)}<span class="fm-tag">{chip}</span>{/each}
                {:else}
                  {parsed.text}
                {/if}
              </dd>
            </div>
          {/each}
        </dl>
      {/if}
      <!-- the backend's sanitized HTML is injected here by DocRenderer; links and code references are handled in onclick -->
      <!-- svelte-ignore a11y_no_static_element_interactions -->
      <div bind:this={body} class="md-body" {onclick} {onkeydown} {oncontextmenu}></div>
      {#if commentBars.length > 0}
        <div class="comment-bars">
          {#each commentBars as bar (bar.id)}
            <button
              type="button"
              class="comment-bar"
              tabindex="-1"
              style:top="{bar.top}px"
              style:height="{bar.height}px"
              title={bar.text}
              aria-label={t("comments.bar", { text: bar.text })}
              onclick={() => app.showComment(bar.id)}
            ></button>
          {/each}
        </div>
      {/if}
    </article>
  </div>
</div>

{#if refMenu}
  <CodeRefMenu menu={refMenu} onclose={() => (refMenu = null)} />
{/if}

{#if missing}
  <Popover
    anchor={missing.anchor}
    variant="bar"
    label={t("link.missing.label")}
    onclose={() => (missing = null)}
  >
    <span style="padding:0 8px">{t("link.missing", { path: missing.path })}</span>
    <button
      type="button"
      class="btn btn--sm"
      onclick={() => {
        const path = missing?.path;
        missing = null;
        if (path) void app.copy(path, "missing");
      }}
    >
      <Icon name="copy" size="sm" />{t("common.copyPath")}
    </button>
  </Popover>
{/if}

{#snippet fileActions(path: string)}
  <button type="button" class="btn" onclick={() => void app.openWithDefaultApp(path)}>
    {t("media.openDefault")}
  </button>
  <button type="button" class="btn" onclick={() => void app.openExternally(path)}>
    {t("doc.openExternal")}
  </button>
  <button type="button" class="btn" onclick={() => void app.copyPath(path)}>
    {ui.flashed === "path" ? t("common.copied") : t("common.copyPath")}
  </button>
{/snippet}
