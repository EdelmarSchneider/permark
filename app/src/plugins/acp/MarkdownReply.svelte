<script lang="ts">
  // An agent's message in the conversation. While it comes: its plain text (markup stays text). Once whole (another
  // line follows it, or the turn ended): the plugin's backend renders it once through the documents' Markdown pipeline,
  // and its sanitized blocks are drawn as the export builder draws blocks off the page (template, enhance, embeds near
  // each block) under the reading view's classes, in the panel's size. An answer for text no longer shown is dropped; a
  // render that fails leaves the text. A message that came whole (a continued conversation replayed) shows an empty
  // line until its blocks, never its raw markup first (its text only if the render fails). The last message never shrinks under the reader at the swap (that floor goes once
  // another line follows it). A click never navigates the window: web links open in the person's browser, project links
  // and code references open the file at the line, a link to a note of the message scrolls to it; a picture on the web
  // stays an inert chip (a reply never fetches it). Its headings sit below the panel's own (h4 and deeper), each keeping
  // its look; code wraps its lines, so no block scrolls sideways (a scroller the keyboard cannot reach in WebKit).
  import type { Attachment } from "svelte/attachments";
  import { untrack } from "svelte";
  import { api, type Block } from "../../api";
  import { conditions } from "../../core/conditions/context.svelte";
  import {
    codeRefOf,
    Decorator,
    type CodeRefTarget,
    type EmbedLookup,
  } from "../../core/doc/decorate";
  import { documents } from "../../core/doc/document.svelte";
  import { enhanceBlock } from "../../core/doc/enhance";
  import "../../core/doc/markdown.css";
  import { contributions } from "../../core/plugins/instance.svelte";
  import { acp } from "./store.svelte";

  type Props = {
    /** The message as it came so far. */
    text: string;
    /** The message is whole. */
    final: boolean;
    /** The message is the conversation's last line: only then does it keep the plain text's height. */
    last?: boolean;
    /** The message came whole (replayed): no plain text before its blocks, only if the render fails. */
    quiet?: boolean;
  };
  let { text, final, last = false, quiet = false }: Props = $props();

  /** The document the backend renders a message as: links and code references resolve from the project root. */
  const DOC = "assistant.md";

  /** What the backend answered for the text it was asked for: its blocks, or none (the render failed). */
  type Answer =
    | { status: "drawn"; text: string; blocks: readonly Block[] }
    | { status: "failed"; text: string };

  let plain = $state<HTMLElement>();
  /** The last answer. */
  let answer = $state.raw<Answer | null>(null);
  /** Height of the plain text at the swap (px): the rendered message is at least as tall. */
  let floor = $state(0);
  /** Counts the asks: an answer to an earlier one is dropped. */
  let generation = 0;
  /** The decorator of the blocks on screen (`null`: none drawn). */
  let decorator: Decorator | null = null;

  const shown = $derived(
    final && answer?.status === "drawn" && answer.text === text ? answer.blocks : null,
  );
  const failed = $derived(answer?.status === "failed" && answer.text === text);
  const embeds = $derived(contributions.visible("embed", conditions.current));
  const lookup: EmbedLookup = $derived(
    (language) => embeds.find((e) => e.language === language)?.view,
  );

  // the whole message is rendered once; a new text asks again and the earlier answer no longer counts
  $effect(() => {
    if (!final) return;
    const asked = text;
    const before = untrack(() => answer);
    if (before?.status === "drawn" && before.text === asked) return;
    const turn = generation;
    // a failed render leaves the plain text (a quiet message shows it from then on)
    const fail = (): void => {
      if (turn === generation) answer = { status: "failed", text: asked };
    };
    acp.render(asked).then((blocks) => {
      if (turn !== generation) return;
      if (blocks === null) {
        fail();
        return;
      }
      floor = plain?.offsetHeight ?? 0;
      answer = { status: "drawn", text: asked, blocks };
    }, fail);
    return () => {
      generation += 1;
    };
  });

  // a plugin with an embed came or went: the blocks on screen follow
  $effect(() => {
    decorator?.setEmbeds(lookup);
  });

  /** Draws `blocks` into the element (sanitized HTML through a template only, as the reading view does). */
  function draw(blocks: readonly Block[]): Attachment<HTMLElement> {
    return (host) => {
      const drawn = new Decorator(api);
      drawn.setDocument(DOC);
      const env = documents.enhanceEnv;
      const template = document.createElement("template");
      const elements: HTMLElement[] = [];
      for (const block of blocks) {
        if (block.html === null) continue;
        template.innerHTML = block.html;
        const first = template.content.firstElementChild;
        if (first instanceof HTMLElement) elements.push(demote(enhanceBlock(first, env)));
      }
      host.replaceChildren(...elements);
      settle(host);
      drawn.setEmbeds(untrack(() => lookup));
      for (const element of elements) drawn.near(element);
      decorator = drawn;
      return () => {
        drawn.dispose();
        if (decorator === drawn) decorator = null;
      };
    };
  }

  /** The block with its headings below the panel's (`h1` -> `h4`, deeper ones `h6`), `data-level` keeping the level
   *  written for the look; the block's root replaced when it is one. */
  function demote(element: HTMLElement): HTMLElement {
    const own = /^H[1-6]$/.test(element.tagName) ? [element] : [];
    let root = element;
    for (const heading of [
      ...own,
      ...element.querySelectorAll<HTMLElement>("h1, h2, h3, h4, h5, h6"),
    ]) {
      const level = Number(heading.tagName.slice(1));
      const lower = document.createElement(`h${String(Math.min(level + 3, 6))}`);
      for (const attribute of [...heading.attributes])
        lower.setAttribute(attribute.name, attribute.value);
      lower.setAttribute("data-level", String(level));
      lower.replaceChildren(...heading.childNodes);
      heading.replaceWith(lower);
      if (heading === element) root = lower;
    }
    return root;
  }

  /** What a message must not do that a document may: fetch a web picture (its chip loses the address it would load,
   *  the address stays its tooltip), or give the window ids (another message | the open document has the same). */
  function settle(host: HTMLElement): void {
    for (const chip of host.querySelectorAll<HTMLElement>(".remote-image[data-url]")) {
      const url = chip.getAttribute("data-url") ?? "";
      chip.removeAttribute("data-url");
      chip.title = url;
    }
    for (const anchor of host.querySelectorAll("a.anchor")) anchor.remove();
    for (const element of host.querySelectorAll("[id]")) {
      element.setAttribute("data-anchor", element.id);
      element.removeAttribute("id");
    }
  }

  function openRef(ref: CodeRefTarget): void {
    const path = ref.paths[0];
    if (path !== undefined) acp.openAt(path, ref.line);
  }

  /** Scrolls to the part of the message `id` names (a footnote). */
  function toPart(host: HTMLElement, id: string): void {
    const part = [...host.querySelectorAll("[data-anchor]")].find(
      (element) => element.getAttribute("data-anchor") === id,
    );
    part?.scrollIntoView({ block: "nearest" });
  }

  async function follow(href: string, host: HTMLElement): Promise<void> {
    const value = href.trim();
    if (value.startsWith("#")) {
      toPart(host, value.slice(1));
      return;
    }
    if (/^(https?:|mailto:)/i.test(value)) {
      acp.openWebsite(value);
      return;
    }
    // any other scheme is never followed from a message
    if (/^[a-z][a-z0-9+.-]*:/i.test(value) || decorator === null) return;
    const [target] = await decorator.links.resolve([value]);
    if (target) acp.openLink(target);
  }

  /** A click | the middle button on a link or a code reference: never the web view's own way. */
  function activate(event: MouseEvent): void {
    const target = event.target;
    const host = event.currentTarget;
    if (!(target instanceof Element) || !(host instanceof HTMLElement)) return;
    const ref = codeRefOf(target);
    if (ref) {
      event.preventDefault();
      openRef(ref);
      return;
    }
    const link = target.closest("a[href]");
    if (!link) return;
    event.preventDefault();
    const href = link.getAttribute("href");
    if (href) void follow(href, host);
  }

  function onauxclick(event: MouseEvent): void {
    if (event.button === 1) activate(event);
  }

  // Enter on a code reference opens it as a click does (a link answers Enter itself)
  function onkeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter" || event.isComposing || !(event.target instanceof Element)) return;
    const ref = codeRefOf(event.target);
    if (!ref) return;
    event.preventDefault();
    openRef(ref);
  }
</script>

{#if shown !== null}
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="md md-body reply"
    style:min-height={last && floor > 0 ? `${String(floor)}px` : undefined}
    onclick={activate}
    {onauxclick}
    {onkeydown}
    {@attach draw(shown)}
  ></div>
{:else if quiet && !failed}
  <div class="pending" aria-busy="true"></div>
{:else}
  <p class="text" bind:this={plain}>{text}</p>
{/if}

<style>
  .text {
    margin: 0;
    font-size: var(--fs-ui-sm);
    line-height: 1.5;
    white-space: pre-wrap;
    overflow-wrap: anywhere;
  }
  /* one line of the plain text held while the blocks come */
  .pending {
    min-height: calc(var(--fs-ui-sm) * 1.5);
  }
  /* the reading view's look in the panel's size: no column width, no page margins, tighter steps */
  .reply {
    --zoom: 1;
    max-width: none;
    margin: 0;
    font-size: var(--fs-ui-sm);
    line-height: 1.5;
    letter-spacing: normal;
    overflow-wrap: anywhere;
  }
  .reply > :global(*) {
    margin: 0 0 0.6em;
  }
  .reply > :global(:last-child) {
    margin-bottom: 0;
  }
  .reply :global(:is(h1, h2, h3, h4, h5, h6)) {
    margin: 0.8em 0 0.4em;
    padding: 0;
    border: 0;
  }
  .reply > :global(:is(h1, h2, h3, h4, h5, h6):first-child) {
    margin-top: 0;
  }
  /* a heading keeps the look of the level written, whatever level it now has */
  .reply :global([data-level]) {
    font-weight: 600;
    letter-spacing: normal;
    color: inherit;
  }
  .reply :global([data-level="1"]) {
    font-size: 1.4em;
    font-weight: 700;
  }
  .reply :global([data-level="2"]) {
    font-size: 1.25em;
    font-weight: 650;
  }
  .reply :global([data-level="3"]) {
    font-size: 1.12em;
  }
  .reply :global(:is([data-level="4"], [data-level="5"], [data-level="6"])) {
    font-size: 1em;
  }
  .reply :global([data-level="6"]) {
    color: var(--muted);
  }
  .reply :global(:is(ul, ol)) {
    padding-inline-start: 1.4em;
  }
  /* code wraps as in documents (a long word too): nothing scrolls sideways in the panel */
  .reply :global(pre) {
    padding: 8px 10px;
    font-size: 0.95em;
  }
  /* the first line starts below the corner (language, Copy), which would cover a line's end in the narrow panel */
  .reply :global(.code-block > pre:not(.has-title)) {
    padding-top: calc(var(--code-corner-top) + var(--icon-btn-sm) + 2px);
  }
  .reply :global(:is(th, td)) {
    padding: 4px 8px;
    min-width: 3em;
    overflow-wrap: normal;
  }
</style>
