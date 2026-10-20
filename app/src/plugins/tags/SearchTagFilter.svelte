<script lang="ts">
  // With the search boxes (Search panel, Quick open, Search everywhere): the tags the search is narrowed to, as chips,
  // and a small list to add another. Core keeps the selection (`searchTags`); this draws it.
  import Plus from "@lucide/svelte/icons/plus";
  import { onMount } from "svelte";
  import { t } from "../../core/i18n";
  import { searchTags } from "../../core/search/tags.svelte";
  import TagChip from "./TagChip.svelte";
  import { tags } from "./store.svelte";

  let open = $state(false);
  let root: HTMLElement | undefined = $state();

  const choices = $derived(
    (tags.overview?.tags ?? [])
      .filter((item) => !searchTags.selected.includes(item.name))
      .slice(0, 40),
  );

  onMount(() => {
    const outside = (event: PointerEvent): void => {
      if (root && event.target instanceof Node && !root.contains(event.target)) open = false;
    };
    // Escape closes the list before it closes the box around it
    const escape = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || !open) return;
      event.preventDefault();
      event.stopPropagation();
      open = false;
    };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", escape, true);
    return () => {
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("keydown", escape, true);
    };
  });
</script>

{#if searchTags.selected.length > 0 || choices.length > 0}
  <div class="filter" bind:this={root} role="group" aria-label={t("tags.filterLabel")}>
    {#each searchTags.selected as name (name)}
      {@const style = tags.styleOf(name)}
      <TagChip
        {name}
        color={style?.color}
        icon={style?.icon}
        onremove={() => searchTags.remove(name)}
        title={t("tags.queryChip", { name })}
      />
    {/each}
    {#if choices.length > 0}
      <span class="add">
        <button
          type="button"
          class="plus"
          aria-expanded={open}
          title={t("tags.addFilterHint")}
          onclick={() => (open = !open)}
        >
          <Plus size={12} aria-hidden="true" />
          {t("tags.addFilter")}
        </button>
        {#if open}
          <ul class="menu" aria-label={t("tags.addFilterHint")}>
            {#each choices as item (item.name)}
              {@const style = tags.styleOf(item.name)}
              <li>
                <button
                  type="button"
                  onclick={() => {
                    searchTags.add(item.name);
                    open = false;
                  }}
                >
                  <TagChip name={item.name} color={style?.color} icon={style?.icon} />
                  <span class="n">{item.count}</span>
                </button>
              </li>
            {/each}
          </ul>
        {/if}
      </span>
    {/if}
  </div>
{/if}

<style>
  .filter {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1);
    padding: var(--sp-1) var(--sp-3);
  }
  .add {
    position: relative;
  }
  .plus {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    padding: 0 var(--sp-2);
    border: 1px dashed var(--border-strong);
    border-radius: 999px;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-xs);
    line-height: 18px;
    cursor: pointer;
  }
  .plus:hover,
  .plus[aria-expanded="true"] {
    color: var(--text);
    border-color: var(--muted);
  }
  .menu {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: 30;
    min-width: 180px;
    max-height: 240px;
    margin: 2px 0 0;
    padding: var(--sp-1);
    overflow-y: auto;
    list-style: none;
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
  }
  .menu button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 3px var(--sp-1);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    cursor: pointer;
  }
  .menu button:hover {
    background: var(--hover);
  }
  .n {
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    font-variant-numeric: tabular-nums;
  }
</style>
