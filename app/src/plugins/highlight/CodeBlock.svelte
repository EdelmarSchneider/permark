<script lang="ts">
  // A fenced code block: the text, highlighted when it can be, the language in the corner, a copy button.
  // Mounted into the block's wrapper when the block comes near the screen.
  import Check from "@lucide/svelte/icons/check";
  import Copy from "@lucide/svelte/icons/copy";
  import { findCodeSecrets, MASK } from "../../core/config/secrets";
  import { t } from "../../core/i18n";
  import type { EmbedProps } from "../../core/plugins/props";
  import { highlightFence, type CodeOutcome } from "./highlighter";
  import "./tokens.css";

  let { source, info }: EmbedProps = $props();

  let outcome = $state.raw<CodeOutcome>({ html: null, label: null });
  let copied = $state(false);
  let revealed = $state.raw<{ source: string; ids: ReadonlySet<number> }>({
    source: "",
    ids: new Set(),
  });

  // Secret values are replaced by one private-use character each before highlighting (it stays one token), and the
  // character becomes a mask button afterwards. `source` stays the real text: copy copies it.
  const FIRST = 0xe100;
  const spans = $derived(findCodeSecrets(source).slice(0, 4000));
  const shown = $derived.by(() => {
    let out = "";
    let at = 0;
    spans.forEach((span, i) => {
      out += source.slice(at, span.from) + String.fromCodePoint(FIRST + i);
      at = span.to;
    });
    return out + source.slice(at);
  });
  const open = $derived(revealed.source === source ? revealed.ids : new Set<number>());

  function escape(text: string): string {
    return text.replace(/&/g, "&amp;").replace(/</g, "&lt;").replace(/>/g, "&gt;");
  }

  /** The highlighted (or plain) text with the placeholders turned into masks, or into the value once revealed. */
  const html = $derived.by(() => {
    if (spans.length === 0) return outcome.html;
    const base = outcome.html ?? escape(shown);
    return base.replace(/[\ue100-\uf8ff]/g, (char) => {
      const i = (char.codePointAt(0) ?? FIRST) - FIRST;
      const span = spans[i];
      if (span === undefined) return char;
      if (open.has(i)) return escape(source.slice(span.from, span.to));
      return `<button type="button" class="secret" data-i="${i}" title="${t("config.reveal")}" aria-label="${t("config.reveal")}">${MASK}</button>`;
    });
  });

  function reveal(event: MouseEvent): void {
    const target = event.target;
    if (!(target instanceof Element)) return;
    const button = target.closest("button.secret");
    const i = Number(button?.getAttribute("data-i"));
    if (button === null || Number.isNaN(i)) return;
    revealed = { source, ids: new Set([...open, i]) };
  }

  // highlight.js loads on first use; a block changed meanwhile ignores the old answer
  $effect(() => {
    let stale = false;
    void highlightFence(shown, info).then((result) => {
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

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
<pre onclick={reveal}><code
    >{#if html !== null}{@html html}{:else}{source}{/if}</code
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
