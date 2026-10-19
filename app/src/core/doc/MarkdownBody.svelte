<script lang="ts">
  // The page of a Markdown document. The blocks live in the DOM (kept by id by the controller); this holds the frame
  // around them: width and zoom, the properties card, link clicks, the selection menu, the picture overlay.
  import type { Component } from "svelte";
  import type { DocumentView } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import type { MenuEntry } from "../explorer/menu";
  import { t } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import type { FrontMatterProps } from "../plugins/props";
  import { api } from "../../api";
  import type { EmbedLookup } from "./decorate";
  import { documents } from "./document.svelte";
  import { reading } from "./reading.svelte";
  import "./markdown.css";

  type Props = {
    view: DocumentView;
    scroller: HTMLElement;
    /** The reference template (setting). */
    template: string;
    deleted: boolean;
  };
  let { view, scroller, template, deleted }: Props = $props();

  let container: HTMLElement | undefined = $state();
  let menu = $state.raw<{ x: number; y: number; entries: readonly MenuEntry[] } | null>(null);

  const embeds = $derived(contributions.visible("embed", conditions.current));
  const lookup: EmbedLookup = $derived(
    (language) => embeds.find((e) => e.language === language)?.view,
  );
  const card = $derived(contributions.visible("frontMatterCard", conditions.current)[0]);
  const front = $derived(view.body.kind === "markdown" ? view.body.frontMatter : null);
  /** A long document lets the webview skip the layout of blocks far from the screen. */
  const long = $derived(view.body.kind === "markdown" && view.body.blocks.length > 500);

  // the page exists while this component does
  $effect(() => {
    if (!container) return;
    documents.attachBody(container, scroller);
    return () => documents.detachBody();
  });

  // the document (or the embeds on offer) changed: the page follows
  $effect(() => {
    void documents.state;
    documents.paint(lookup);
  });

  // where the reader is: remembered, and the heading found (one frame at a time)
  $effect(() => {
    let frame = 0;
    const onScroll = (): void => {
      if (frame !== 0) return;
      frame = requestAnimationFrame(() => {
        frame = 0;
        documents.noteScroll();
      });
    };
    scroller.addEventListener("scroll", onScroll, { passive: true });
    return () => {
      scroller.removeEventListener("scroll", onScroll);
      if (frame !== 0) cancelAnimationFrame(frame);
    };
  });

  function onclick(event: MouseEvent): void {
    const target = event.target;
    if (!(target instanceof Element) || event.button !== 0) return;
    const ref = documents.codeRefAt(target);
    if (ref) {
      event.preventDefault();
      openRef(ref.paths, ref.line, ref.col, event);
      return;
    }
    const link = target.closest("a[href]");
    if (link) {
      const href = link.getAttribute("href");
      if (href) {
        event.preventDefault();
        void documents.activateLink(href);
      }
      return;
    }
    const chip = target.closest(".remote-image");
    if (chip) {
      const url = chip.getAttribute("data-url");
      if (url) void documents.openWeb(url);
      return;
    }
    if (target instanceof HTMLImageElement && !target.classList.contains("is-broken")) {
      documents.zoomed = { src: target.currentSrc || target.src, alt: target.alt };
    }
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter" || event.isComposing) return;
    const target = event.target;
    if (!(target instanceof Element)) return;
    const ref = documents.codeRefAt(target);
    if (ref) {
      event.preventDefault();
      openRef(ref.paths, ref.line, ref.col, event);
    }
  }

  /** One file: the editor at the line. Several: a menu of them. */
  function openRef(
    paths: string[],
    line: number | null,
    col: number | null,
    event: MouseEvent | KeyboardEvent,
  ): void {
    const [only] = paths;
    if (paths.length === 1 && only !== undefined) {
      void documents.openCodeRef({ paths, line, col }, only);
      return;
    }
    const at =
      event instanceof MouseEvent
        ? { x: event.clientX, y: event.clientY }
        : { x: window.innerWidth / 2, y: window.innerHeight / 3 };
    menu = {
      ...at,
      entries: paths.map((path) => ({
        kind: "item",
        label: line === null ? path : `${path}:${line}`,
        run: () => void documents.openCodeRef({ paths, line, col }, path),
      })),
    };
  }

  function oncontextmenu(event: MouseEvent): void {
    const selection = window.getSelection();
    const text = selection && !selection.isCollapsed ? selection.toString() : "";
    const entries: MenuEntry[] = [];
    if (text !== "") {
      const anchor = selection?.anchorNode ?? null;
      entries.push(
        { kind: "item", label: t("selection.copy"), run: () => void documents.copyPlain(text) },
        {
          kind: "item",
          label: t("selection.copyWithReference"),
          run: () => void documents.copySelection(text, anchor, template),
        },
      );
    } else {
      entries.push(
        {
          kind: "item",
          label: t("header.copyReference"),
          run: () => void documents.copyReference(template),
        },
        { kind: "item", label: t("cmd.copyDocument"), run: () => void documents.copyDocument() },
      );
    }
    event.preventDefault();
    menu = { x: event.clientX, y: event.clientY, entries };
  }

  // the card is a plugin's: loaded when it is on
  const cardView: Promise<{ default: Component<FrontMatterProps> }> | null = $derived(
    card ? card.view() : null,
  );
  const cardProps: FrontMatterProps | null = $derived(
    front === null
      ? null
      : {
          syntax: front.syntax === "toml" ? "toml" : "yaml",
          raw: front.raw,
          fields: front.fields,
        },
  );
</script>

<div class="doc-column">
  <article
    class="md"
    class:is-wide={reading.width === "wide"}
    class:is-full={reading.width === "full"}
    class:is-long={long}
    class:is-tombstone={deleted}
    style:--zoom={reading.zoom / 100}
    lang={undefined}
  >
    {#if cardView !== null && cardProps !== null}
      {#await cardView then loaded}
        {@const Card = loaded.default}
        <Card {...cardProps} />
      {/await}
    {/if}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
    <div
      class="md-body"
      bind:this={container}
      {onclick}
      {onkeydown}
      {oncontextmenu}
      role="document"
    ></div>
  </article>
</div>

{#if menu}
  <ContextMenu x={menu.x} y={menu.y} entries={menu.entries} close={() => (menu = null)} />
{/if}

<style>
  .md-body {
    min-width: 0;
  }
</style>
