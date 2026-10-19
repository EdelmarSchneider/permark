<script lang="ts">
  // A fenced code block: the text, highlighted when it can be, the language in the corner, a copy button.
  // Mounted into the block's wrapper when the block comes near the screen.
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import { t } from "../../core/i18n";
  import type { EmbedProps } from "../../core/plugins/props";
  import { highlightFence, type CodeOutcome } from "./highlighter";
  import "./tokens.css";

  let { source, info }: EmbedProps = $props();

  let outcome = $state.raw<CodeOutcome>({ html: null, label: null });
  let copied = $state(false);

  // highlight.js loads on first use; a block changed meanwhile ignores the old answer
  $effect(() => {
    let stale = false;
    void highlightFence(source, info).then((result) => {
      if (!stale) outcome = result;
    });
    return () => {
      stale = true;
    };
  });

  // the check mark goes back to the copy icon
  $effect(() => {
    if (!copied) return;
    const timer = setTimeout(() => (copied = false), 1500);
    return () => clearTimeout(timer);
  });

  async function copy(): Promise<void> {
    try {
      await navigator.clipboard.writeText(source);
      copied = true;
    } catch {
      // the system refused the clipboard: nothing else to do
    }
  }
</script>

<pre><code
    >{#if outcome.html !== null}{@html outcome.html}{:else}{source}{/if}</code
  ></pre>
{#if outcome.label}
  <span
    class="lang"
    title={outcome.label.auto ? t("doc.codeAutoHint") : undefined}
    class:is-guess={outcome.label.auto}
  >
    {outcome.label.auto ? t("doc.codeAuto", { name: outcome.label.name }) : outcome.label.name}
  </span>
{/if}
<button
  type="button"
  class="icon-btn icon-btn--sm copy"
  class:is-done={copied}
  aria-label={t("doc.copyCode")}
  title={t("doc.copyCode")}
  onclick={() => void copy()}
>
  {#if copied}<Check size={14} />{:else}<Copy size={14} />{/if}
</button>

<style>
  /* the language in the corner, left of the copy button; muted (not faint) so it keeps AA contrast on the surface */
  .lang {
    position: absolute;
    top: 3px;
    right: 42px;
    padding: 0 4px;
    border-radius: 4px;
    background: var(--surface);
    font: 11px/1.3 var(--font-code);
    color: var(--muted);
    pointer-events: none;
    user-select: none;
  }
  .lang.is-guess {
    pointer-events: auto;
    cursor: help;
  }
  .copy {
    position: absolute;
    top: 5px;
    right: 6px;
    opacity: 0;
    transition: opacity var(--dur-fast);
  }
  :global(.code-block:hover) .copy,
  .copy:focus-visible,
  .copy.is-done {
    opacity: 1;
  }
</style>
