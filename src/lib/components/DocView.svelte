<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { bridge } from "../bridge";
  import { DomFind } from "../dom/domFind";
  import { DocRenderer, type RenderLabels, type RenderResult } from "../dom/docRender";
  import { anchorTarget, findById, selectionIn } from "../dom/domUtil";
  import { readBlocks } from "../dom/outlineDom";
  import { enhanceContextFor } from "../dom/enhanceContext";
  import type { EnhanceContext } from "../dom/enhance";
  import { ICON_CHECK, ICON_COPY } from "../dom/enhance";
  import { frontmatterValue } from "../frontmatter";
  import { t, tn } from "../i18n";
  import { buildOutline, currentHeading } from "../outline";
  import { assetUrl } from "../platform";
  import { basename, decodeFragment } from "../paths";
  import { blockForLine, parseSourcepos } from "../sourcepos";
  import { app } from "../stores/app.svelte";
  import { docs, type LocateRequest, type RenderRequest } from "../stores/docs.svelte";
  import { outline } from "../stores/outline.svelte";
  import { session } from "../stores/session.svelte";
  import { toast } from "../stores/toast.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import { clock as formatClock } from "../time";
  import type { DocumentView } from "../types";
  import Icon from "./Icon.svelte";
  import Notice from "./Notice.svelte";
  import DocMetaStrip from "./DocMetaStrip.svelte";
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

  const view = $derived(docs.current);
  const showArticle = $derived(!!view && view.kind !== "binary" && view.kind !== "tooLarge");
  const tombstone = $derived(docs.deleted !== null);
  // `tags:` moved to the strip above the document
  const frontmatter = $derived(
    (view?.frontmatter ?? []).filter(([key]) => key.toLowerCase() !== "tags"),
  );

  // ---- rendering ----------------------------------------------------------------------------------

  const enhanceContext = (path: string): EnhanceContext =>
    enhanceContextFor(path, session.resolvedTheme);

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

    if (view.kind === "binary" || view.kind === "tooLarge") {
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
      if (renderedRev === req.rev) updateCurrent();
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
    scrollToElement(target, 1 / 3);
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

  // ---- effects ----------------------------------------------------------------------------------------

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

  let lastTheme = untrack(() => session.resolvedTheme);
  $effect(() => {
    const theme = session.resolvedTheme;
    untrack(() => {
      if (theme === lastTheme) return;
      lastTheme = theme;
      if (view && renderer) void renderer.rethemeDiagrams(enhanceContext(view.path));
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
        if (el.getBoundingClientRect().bottom <= edge + 1) continue;
        return parseSourcepos(el.getAttribute("data-sourcepos"))?.startLine ?? null;
      }
      return null;
    };
    return () => {
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

  async function onclick(e: MouseEvent): Promise<void> {
    const target = e.target as Element;
    const copy = target.closest<HTMLElement>(".code-block__copy");
    if (copy) {
      void copyCode(copy);
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
  class="doc-scroll"
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
      <!-- the backend's sanitized HTML is injected here by DocRenderer; links are handled in onclick -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div bind:this={body} class="md-body" {onclick}></div>
    </article>
  </div>
</div>

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
  <button type="button" class="btn" onclick={() => void app.openExternally(path)}>
    {t("doc.openExternal")}
  </button>
  <button type="button" class="btn" onclick={() => void app.copyPath(path)}>
    {ui.flashed === "path" ? t("common.copied") : t("common.copyPath")}
  </button>
{/snippet}
