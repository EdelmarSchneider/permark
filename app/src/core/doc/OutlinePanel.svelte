<script lang="ts">
  // The outline of the document in the right panel: the headings, indented by structure, the one the reader is at
  // marked. A click goes to the heading.
  import type { OutlineItem } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import { t } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import type { OutlineCell } from "../plugins/registry.svelte";
  import { documents } from "./document.svelte";
  import { depths } from "./outline";

  type Props = { items: readonly OutlineItem[] };
  let { items }: Props = $props();

  const indent = $derived(depths(items));
  let list: HTMLElement | undefined = $state();
  // columns plugins add (the tokens of each section): only those with something to show take room
  const columns = $derived(contributions.visible("outlineColumn", conditions.current));
  const headers = $derived(columns.map((column) => ({ column, header: column.header() })));
  const shown = $derived(headers.filter((h) => h.header !== null));
  // marks plugins put on changed sections (a dot after the label): the first one that says something
  const marks = $derived(contributions.visible("outlineMark", conditions.current));
  function markOf(at: number): OutlineCell | null {
    for (const mark of marks) {
      const found = mark.mark(at);
      if (found !== null) return found;
    }
    return null;
  }

  // the current heading stays in view in a long outline
  $effect(() => {
    const at = documents.currentHeading;
    if (at < 0 || !list) return;
    list.querySelector<HTMLElement>(`[data-at="${at}"]`)?.scrollIntoView({ block: "nearest" });
  });
</script>

<section class="outline" aria-label={t("outline.label")}>
  <h2 class="title">
    <span>{t("outline.label")}</span>
    {#each shown as { column, header } (column.owner + column.id)}
      <span class="column-head" title={header?.title}>{header?.text}</span>
    {/each}
  </h2>
  {#each columns as column (column.owner + column.id)}
    {#await column.watcher() then loaded}
      {@const Watcher = loaded.default}
      <Watcher {items} />
    {/await}
  {/each}
  {#if items.length === 0}
    <p class="empty">{t("outline.empty")}</p>
  {:else}
    <ol class="items" bind:this={list}>
      {#each items as item, at (at)}
        {@const mark = markOf(at)}
        <li style:--depth={indent[at] ?? 0} class:with-columns={shown.length > 0}>
          <button
            type="button"
            class="item"
            class:is-current={documents.currentHeading === at}
            data-at={at}
            title={mark?.title === undefined ? item.label : `${item.label} · ${mark.title}`}
            aria-current={documents.currentHeading === at ? "location" : undefined}
            onclick={() => documents.scrollToAnchor(item.id)}
          >
            <span class="text">{item.label === "" ? t("outline.untitled") : item.label}</span>
            {#if mark !== null}<span class="mark" data-mark
                ><span class="sr-only">{mark.text}</span></span
              >{/if}
          </button>
          {#each shown as { column } (column.owner + column.id)}
            {@const cell = column.cell(at)}
            <span class="cell" title={cell?.title}>{cell?.text ?? ""}</span>
          {/each}
        </li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .outline {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
  }
  .title {
    display: flex;
    align-items: baseline;
    justify-content: space-between;
    gap: var(--sp-2);
    margin: 0;
    padding: var(--sp-3) var(--sp-3) var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.05em;
    text-transform: uppercase;
  }
  .empty {
    margin: 0;
    padding: var(--sp-2) var(--sp-3);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .items {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 0 var(--sp-2) var(--sp-4);
    overflow-y: auto;
    list-style: none;
  }
  li.with-columns {
    display: flex;
    align-items: baseline;
    gap: var(--sp-1);
  }
  .column-head,
  .cell {
    flex: none;
    min-width: 5ch;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .cell {
    padding-right: var(--sp-2);
    color: var(--faint);
    font-size: var(--fs-ui-sm);
  }
  .item {
    display: flex;
    align-items: center;
    gap: 6px;
    flex: 1;
    min-width: 0;
    width: 100%;
    padding: 3px var(--sp-2);
    padding-left: calc(var(--sp-2) + var(--depth, 0) * 12px);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    font-size: var(--fs-ui);
    line-height: 1.4;
    text-align: left;
    white-space: nowrap;
    overflow: hidden;
    cursor: pointer;
  }
  .text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* a changed section: a small dot after the label (the accent: the glow's own tint is too faint for a dot) */
  .mark {
    flex: none;
    width: 6px;
    height: 6px;
    border-radius: 50%;
    background: var(--accent);
  }
  .item:hover {
    background: var(--hover);
    color: var(--text);
  }
  .item.is-current {
    background: var(--accent-soft);
    color: var(--accent-text);
    font-weight: 600;
  }
</style>
