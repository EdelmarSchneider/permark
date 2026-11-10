<script lang="ts">
  // One text of the translation excerpt under its name ("Original", "Translation · German"): cut to three lines,
  // with Show more | Show less once it is longer than that. The text is a button: a click shows the suggestion in the
  // document. It is read in its own language (`lang`) and laid out by its own direction.
  import { t } from "../../core/i18n";

  type Props = {
    /** `original`: quiet; `translation`: on the color of added text. */
    kind: "original" | "translation";
    /** The block's name. */
    label: string;
    text: string;
    /** The language of the text (`null`: not known). */
    lang: string | null;
    /** Shows the suggestion in the document. */
    show: () => void;
  };
  let { kind, label, text, lang, show }: Props = $props();

  let body: HTMLElement | undefined = $state();
  let expanded = $state(false);
  /** The text is longer than the three lines it is cut to. */
  let long = $state(false);

  // read from the layout while the text is cut (shown in full, the last answer stands): it changes with the text and
  // with the bar's width (observer torn down with the block; none = measured once, as in tests)
  $effect(() => {
    void text;
    const el = body;
    if (!el || expanded) return;
    const measure = (): void => {
      long = el.scrollHeight > el.clientHeight + 1;
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(measure);
    observer.observe(el);
    return () => observer.disconnect();
  });
</script>

<div class={["block", kind]} data-block={kind}>
  <span class="label">{label}</span>
  <button
    bind:this={body}
    type="button"
    class="text"
    class:cut={!expanded}
    data-action="show"
    data-text
    title={t("translation.show")}
    lang={lang ?? undefined}
    dir="auto"
    onclick={show}>{text}</button
  >
  {#if long}
    <button
      type="button"
      class="more"
      data-action="more"
      aria-expanded={expanded}
      onclick={() => (expanded = !expanded)}
      >{t(expanded ? "translation.showLess" : "translation.showMore")}</button
    >
  {/if}
</div>

<style>
  .block {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .label {
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
  }
  .text {
    margin: 0 calc(-1 * var(--sp-1));
    padding: 2px var(--sp-1);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: var(--fs-ui);
    text-align: start;
    cursor: pointer;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .text.cut {
    display: -webkit-box;
    overflow: hidden;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: 3;
    line-clamp: 3;
  }
  .original .text {
    color: var(--muted);
  }
  .translation .text {
    background: var(--review-add-bg);
  }
  .text:hover {
    box-shadow: inset 0 0 0 1px var(--border-strong);
  }
  .more {
    align-self: flex-start;
    padding: 0;
    border: 0;
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .more:hover {
    text-decoration: underline;
  }
</style>
