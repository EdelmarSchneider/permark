<script lang="ts">
  // Structured text (JSON, XML) as a read-only tree, with the text itself one click away. A text that does not parse
  // is shown as text with a note. Editing comes with the editing area (Source).
  import { t } from "../i18n";
  import TreeRow from "./TreeRow.svelte";
  import type { TreeNode } from "./tree";

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

  const parsed = $derived(parse(text));
  const mode = $derived<Mode>(parsed.ok ? chosen : "source");
</script>

<div class="structured">
  <div class="bar">
    {#if parsed.ok}
      <div class="segments" role="radiogroup" aria-label={t("structured.mode")}>
        {#each ["tree", "source"] as const as option (option)}
          <button
            type="button"
            class="segment"
            role="radio"
            aria-checked={mode === option}
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
    {:else}
      <span class="note">{t("structured.invalid")}</span>
    {/if}
  </div>
  {#if mode === "tree" && parsed.ok}
    <div class="tree" role="tree" aria-label={t("structured.tree")}>
      <TreeRow node={parsed.root} depth={0} open {plain} />
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
    align-items: center;
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
  .segment[aria-checked="true"] {
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
  .interpret {
    height: 24px;
    margin-left: 12px;
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
