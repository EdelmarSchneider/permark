<script lang="ts">
  // Structured text (JSON, XML) as a read-only tree, with the text itself one click away. A text that does not parse
  // is shown as text with a note. Editing comes with the editing area (Source).
  import { t } from "../i18n";
  import SpreadButtons from "./SpreadButtons.svelte";
  import TreeRow from "./TreeRow.svelte";
  import { nextAsk, spreadOf, type Ask, type TreeNode } from "./tree";

  type Props = {
    text: string;
    parse: (text: string) => { ok: true; root: TreeNode } | { ok: false };
    strings?: boolean;
  };
  let { text, parse, strings = false }: Props = $props();

  type Mode = "tree" | "source";
  let chosen = $state<Mode>("tree");
  /** String values as interpreted text. Starts off; not kept across a remount. */
  let plain = $state(false);
  /** The last "Expand all" | "Collapse all"; before one, a small document opens fully. */
  let ask = $state<Ask | null>(null);

  const parsed = $derived(parse(text));
  const mode = $derived<Mode>(parsed.ok ? chosen : "source");
</script>

<div class="structured">
  <div class="bar">
    {#if parsed.ok}
      <!-- two toggle buttons, as the document's mode switch: a radio group would owe arrow keys -->
      <div class="segments" role="group" aria-label={t("structured.mode")}>
        {#each ["tree", "source"] as const as option (option)}
          <button
            type="button"
            class="segment"
            aria-pressed={mode === option}
            onclick={() => (chosen = option)}
          >
            {t(option === "tree" ? "structured.tree" : "structured.source")}
          </button>
        {/each}
      </div>
      {#if strings}
        <button
          type="button"
          class="interpret"
          aria-pressed={plain}
          onclick={() => (plain = !plain)}
        >
          {t("json.interpret")}
        </button>
      {/if}
      {#if mode === "tree"}
        <SpreadButtons onask={(open) => (ask = nextAsk(ask, open))} />
      {/if}
    {:else}
      <span class="note">{t("structured.invalid")}</span>
    {/if}
  </div>
  {#if mode === "tree" && parsed.ok}
    <!-- rows are disclosure buttons (aria-expanded), not tree items: a group, so no tree role without its items -->
    <div class="tree" role="group" aria-label={t("structured.tree")}>
      <TreeRow node={parsed.root} depth={0} spread={spreadOf(parsed.root, ask)} {plain} />
    </div>
  {:else}
    <pre class="source"><code>{text}</code></pre>
  {/if}
</div>

<style>
  .structured {
    padding: var(--sp-3) var(--sp-5) var(--sp-8);
  }
  .bar {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 12px;
    min-height: 28px;
    margin-bottom: var(--sp-2);
  }
  .segments {
    display: inline-grid;
    grid-template-columns: repeat(2, auto);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    overflow: hidden;
  }
  .segment {
    height: 24px;
    padding: 0 12px;
    border: 0;
    border-right: 1px solid var(--border);
    background: var(--bg);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .segment:last-child {
    border-right: 0;
  }
  .segment[aria-pressed="true"] {
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .interpret {
    height: 24px;
    padding: 0 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .interpret[aria-pressed="true"] {
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .note {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .source {
    margin: 0;
    padding: 12px 14px;
    overflow-x: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    font: var(--fs-code) / 1.6 var(--font-code);
    white-space: pre;
  }
</style>
