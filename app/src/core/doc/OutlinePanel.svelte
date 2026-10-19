<script lang="ts">
  // The outline of the document in the right panel: the headings, indented by structure, the one the reader is at
  // marked. A click goes to the heading.
  import type { OutlineItem } from "../../api";
  import { t } from "../i18n";
  import { documents } from "./document.svelte";
  import { depths } from "./outline";

  type Props = { items: readonly OutlineItem[] };
  let { items }: Props = $props();

  const indent = $derived(depths(items));
  let list: HTMLElement | undefined = $state();

  // the current heading stays in view in a long outline
  $effect(() => {
    const at = documents.currentHeading;
    if (at < 0 || !list) return;
    list.querySelector<HTMLElement>(`[data-at="${at}"]`)?.scrollIntoView({ block: "nearest" });
  });
</script>

<section class="outline" aria-label={t("outline.label")}>
  <h2 class="title">{t("outline.label")}</h2>
  {#if items.length === 0}
    <p class="empty">{t("outline.empty")}</p>
  {:else}
    <ol class="items" bind:this={list}>
      {#each items as item, at (at)}
        <li style:--depth={indent[at] ?? 0}>
          <button
            type="button"
            class="item"
            class:is-current={documents.currentHeading === at}
            data-at={at}
            title={item.label}
            aria-current={documents.currentHeading === at ? "location" : undefined}
            onclick={() => documents.scrollToAnchor(item.id)}
          >
            {item.label === "" ? t("outline.untitled") : item.label}
          </button>
        </li>
      {/each}
    </ol>
  {/if}
</section>

<style>
  .outline {
    display: flex;
    flex-direction: column;
    min-height: 0;
    height: 100%;
  }
  .title {
    margin: 0;
    padding: var(--sp-3) var(--sp-3) var(--sp-2);
    color: var(--faint);
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
  .item {
    display: block;
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
    text-overflow: ellipsis;
    cursor: pointer;
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
