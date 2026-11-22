<script lang="ts">
  // A fenced code block: the text, highlighted when it can be, the language in the corner, a copy button; the title,
  // line numbers and marked lines its info string asks for. Mounted into the block's wrapper when the block comes near
  // the screen.
  import CodeActions from "../../core/doc/CodeActions.svelte";
  import CopyButton from "../../core/doc/CopyButton.svelte";
  import { t } from "../../core/i18n";
  import type { EmbedProps } from "../../core/plugins/props";
  import { highlightFence, type CodeOutcome } from "./highlighter";
  import { leadCols, lineSpans, markedLines } from "./htmlLines";
  import "./tokens.css";

  let { source, info, lines = false, region, title, start, marked }: EmbedProps = $props();

  let outcome = $state.raw<CodeOutcome>({ html: null, label: null });
  /** The first answer of the highlighter came: a copy of the page taken now (export) holds the colors. */
  let settled = $state(false);

  function escape(text: string): string {
    return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  const lineCount = $derived(source.split("\n").length);
  const marks = $derived(marked === undefined ? new Set<number>() : markedLines(marked, lineCount));
  /** Digits of the highest line number (the width of the numbers' column). */
  const digits = $derived(start === undefined ? 0 : String(start + lineCount - 1).length);

  /** One element per line when the viewer asks for it or lines are numbered | marked (the plain text too, before
   *  highlighting). The numbers are drawn by the style: never text to select or copy. */
  const drawn = $derived(
    lines || start !== undefined || marks.size > 0
      ? lineSpans(outcome.html ?? escape(source), source.split("\n").map(leadCols), marks)
      : outcome.html,
  );

  // highlight.js loads on first use; a block changed meanwhile ignores the old answer
  $effect(() => {
    let stale = false;
    void highlightFence(source, info)
      .then((result) => {
        if (!stale) outcome = result;
      })
      .finally(() => {
        if (!stale) settled = true;
      });
    return () => {
      stale = true;
    };
  });
</script>

{#if title}<div class="code-title">{title}</div>{/if}
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<pre
  class:is-numbered={start !== undefined}
  class:has-title={!!title}
  style:--ln-start={start === undefined ? undefined : start - 1}
  style:--ln-digits={start === undefined ? undefined : digits}
  tabindex={region === undefined ? undefined : 0}
  role={region === undefined ? undefined : "region"}
  aria-label={region}
  aria-busy={settled ? undefined : "true"}><code
    >{#if drawn !== null}{@html drawn}{:else}{source}{/if}</code
  ></pre>
<div class="corner">
  {#if outcome.label}
    <span
      class="lang"
      title={outcome.label.auto ? t("doc.codeAutoHint") : undefined}
      class:is-guess={outcome.label.auto}
    >
      {outcome.label.auto ? t("doc.codeAuto", { name: outcome.label.name }) : outcome.label.name}
    </span>
  {/if}
  {#if info !== ""}<CodeActions {source} language={info} />{/if}
  <CopyButton copy={source} label={t("doc.copyCode")} class="copy" />
</div>

<style>
  /* the title over the block: one box with the code below it */
  .code-title {
    padding: 6px 16px;
    border: 1px solid var(--border);
    border-bottom: 0;
    border-radius: var(--radius-card) var(--radius-card) 0 0;
    background: var(--surface);
    font: var(--code-label-fs) / var(--code-label-lh) var(--font-code);
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  pre.has-title {
    border-top-left-radius: 0;
    border-top-right-radius: 0;
    border-top-style: dashed;
  }
  /* numbered | marked lines: one block per line; a number sits in the line's own room at the left (`--ln-pad`). A
     wrapped line's further rows stay in the code column, 2 columns right of the line's own indentation (`--lead`). */
  pre :global(.ln) {
    display: block;
    position: relative;
    --hang: calc((2 + var(--lead, 0)) * 1ch);
    padding-left: calc(var(--ln-pad, 0px) + var(--hang));
    text-indent: calc(-1 * var(--hang));
  }
  pre.is-numbered {
    counter-reset: ln var(--ln-start);
    --ln-pad: calc(var(--ln-digits) * 1ch + 14px);
  }
  pre.is-numbered :global(.ln) {
    counter-increment: ln;
  }
  pre.is-numbered :global(.ln::before) {
    content: counter(ln);
    position: absolute;
    left: var(--ln-edge, 0px);
    width: calc(var(--ln-digits) * 1ch);
    text-align: right;
    text-indent: 0;
    color: var(--faint);
    user-select: none;
  }
  /* a marked line is tinted from edge to edge of the block */
  pre :global(.ln.is-marked) {
    --ln-edge: 16px;
    margin-inline: -16px;
    padding-inline: calc(16px + var(--ln-pad, 0px) + var(--hang)) 16px;
    background: color-mix(in srgb, var(--accent) 12%, transparent);
    box-shadow: inset 3px 0 0 var(--accent);
  }
  /* the language, the plugins' buttons and Copy in the corner */
  .corner {
    position: absolute;
    top: var(--code-corner-top);
    right: var(--code-corner-right);
    display: flex;
    align-items: center;
    gap: var(--code-corner-gap);
  }
  /* muted (not faint) so it keeps AA contrast on the surface */
  .lang {
    margin-right: var(--code-label-gap);
    padding: var(--code-label-pad);
    border-radius: var(--code-label-radius);
    background: var(--surface);
    font: var(--code-label-fs) / var(--code-label-lh) var(--font-code);
    color: var(--muted);
    pointer-events: none;
    user-select: none;
  }
  .lang.is-guess {
    pointer-events: auto;
    cursor: help;
  }
  .corner :global(.copy),
  .corner :global(.code-action) {
    opacity: 0;
    transition: opacity var(--dur-fast);
  }
  /* shown while the block is hovered, the button focused, or a copy result is on it */
  :global(.code-block:hover) .corner :global(.copy),
  :global(.code-block:hover) .corner :global(.code-action),
  .corner :global(.code-action:focus-visible),
  .corner :global(.copy:focus-visible),
  .corner :global(.copy.is-done),
  .corner :global(.copy.is-failed) {
    opacity: 1;
  }
</style>
