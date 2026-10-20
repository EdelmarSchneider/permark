<script lang="ts">
  // Under the document header: the file's tags and its one-line description, when it has some. A tag opens the tags view
  // on it; the pencil opens the editor (T).
  import Pencil from "@lucide/svelte/icons/pencil";
  import { documents } from "../../core/doc/document.svelte";
  import { t } from "../../core/i18n";
  import { ui } from "../../core/shell/ui.svelte";
  import TagChip from "./TagChip.svelte";
  import { PANEL } from "./ids";
  import { tags } from "./store.svelte";

  const path = $derived(documents.shown);
  const names = $derived(path === null ? [] : tags.tagsOf(path));
  const description = $derived(path === null ? "" : tags.descriptionOf(path));

  function show(name: string, event: MouseEvent): void {
    tags.pick(name, event.ctrlKey || event.metaKey);
    ui.showSidebar(PANEL);
  }
</script>

{#if path !== null && (names.length > 0 || description !== "")}
  <div class="strip" role="group" aria-label={t("tags.label")}>
    {#each names as name (name)}
      {@const style = tags.styleOf(name)}
      <TagChip
        {name}
        color={style?.color}
        icon={style?.icon}
        onclick={(event) => show(name, event)}
        title={t("tags.chipHint", { name })}
      />
    {/each}
    {#if description !== ""}<span class="description">{description}</span>{/if}
    <span class="spacer"></span>
    <button
      type="button"
      class="edit"
      title={t("tags.addHint")}
      aria-label={t("tags.add")}
      onclick={() => tags.openEditor(path)}
    >
      <Pencil size={13} />
    </button>
  </div>
{/if}

<style>
  .strip {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: var(--sp-1) var(--sp-2);
    padding: var(--sp-1) var(--sp-4);
    border-bottom: 1px solid var(--border);
    background: var(--surface);
    font-size: var(--fs-ui-sm);
  }
  .description {
    min-width: 0;
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .spacer {
    flex: 1;
  }
  .edit {
    display: inline-grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--muted);
    cursor: pointer;
  }
  .edit:hover {
    background: var(--hover);
    color: var(--text);
  }
</style>
