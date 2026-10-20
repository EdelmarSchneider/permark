<script lang="ts">
  // One row of a read-only tree and, when open, its children (made on first open, shown in chunks).
  import ChevronRight from "@lucide/svelte/icons/chevron-right";
  import { t } from "../i18n";
  import TreeRow from "./TreeRow.svelte";
  import { CHUNK, type TreeNode } from "./tree";

  type Props = { node: TreeNode; depth: number; open?: boolean };
  let { node, depth, open = false }: Props = $props();

  /** The person's choice; until then the row follows `open`. */
  let choice = $state<boolean | null>(null);
  let shown = $state(CHUNK);

  const isOpen = $derived(node.children !== null && (choice ?? open));
  // made when the row is open (the first time, and again after a close: cheap, and a big file keeps no copy)
  const children = $derived(isOpen && node.children !== null ? node.children() : []);

  function toggle(): void {
    if (node.children !== null) choice = !isOpen;
  }
</script>

<div class="row" style:--depth={depth}>
  {#if node.children !== null}
    <button type="button" class="head" aria-expanded={isOpen} onclick={toggle}>
      <ChevronRight class={["chevron", isOpen && "is-open"]} size={13} />
      {#if node.label !== null}<span class="label">{node.label}</span>{/if}
      <span class="summary kind-{node.kind}">{node.summary}</span>
    </button>
  {:else}
    <div class="head leaf">
      <span class="chevron-gap"></span>
      {#if node.label !== null}<span class="label">{node.label}</span>{/if}
      <span class="summary kind-{node.kind}">{node.summary}</span>
    </div>
  {/if}
  {#if isOpen}
    <div class="children">
      {#each children.slice(0, shown) as child, at (at)}
        <TreeRow node={child} depth={depth + 1} />
      {/each}
      {#if children.length > shown}
        <button type="button" class="more" onclick={() => (shown += CHUNK)}>
          {t("tree.showMore", {
            n: Math.min(CHUNK, children.length - shown),
            total: children.length - shown,
          })}
        </button>
      {/if}
    </div>
  {/if}
</div>

<style>
  .row {
    font: 13px/1.55 var(--font-code);
  }
  .head {
    display: flex;
    align-items: baseline;
    gap: 6px;
    width: 100%;
    padding: 0 0 0 calc(var(--depth, 0) * 16px);
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .head.leaf {
    cursor: default;
  }
  .head:hover:not(.leaf) {
    background: var(--hover);
  }
  :global(.chevron) {
    flex: none;
    align-self: center;
    color: var(--faint);
    transition: transform var(--dur-fast) var(--ease);
  }
  :global(.chevron.is-open) {
    transform: rotate(90deg);
  }
  .chevron-gap {
    flex: none;
    width: 13px;
  }
  .label {
    flex: none;
    color: var(--syn-prop);
  }
  .label::after {
    content: ":";
    color: var(--muted);
  }
  .summary {
    min-width: 0;
    overflow-wrap: anywhere;
    white-space: pre-wrap;
  }
  .kind-string,
  .kind-text {
    color: var(--syn-string);
  }
  .kind-number,
  .kind-boolean,
  .kind-null {
    color: var(--syn-keyword);
  }
  .kind-object,
  .kind-array,
  .kind-element {
    color: var(--muted);
  }
  .kind-comment {
    color: var(--syn-comment);
    font-style: italic;
  }
  .kind-attribute {
    color: var(--syn-type);
  }
  .more {
    margin: 2px 0 2px calc((var(--depth, 0) + 1) * 16px + 19px);
    padding: 0 8px;
    border: 1px dashed var(--border-strong);
    border-radius: 10px;
    background: transparent;
    color: var(--muted);
    font: 12px var(--font-ui);
    cursor: pointer;
  }
  .more:hover {
    color: var(--text);
    border-color: var(--muted);
  }
</style>
