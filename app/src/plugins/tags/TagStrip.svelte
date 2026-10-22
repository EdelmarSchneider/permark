<script lang="ts">
  // Under the document header: the file's tags and its one-line description, when it has some. A tag opens the tags view
  // on it; the pencil opens the editor (T). Where the opened folder has official lists, each tag's mark is the one of
  // this file's project (asked once per file and change; never while the lists are dormant).
  import Pencil from "@lucide/svelte/icons/pencil";
  import type { TagRefView } from "../../api";
  import { documents } from "../../core/doc/document.svelte";
  import { t } from "../../core/i18n";
  import { ui } from "../../core/shell/ui.svelte";
  import TagChip from "./TagChip.svelte";
  import { PANEL } from "./ids";
  import { tags } from "./store.svelte";

  const path = $derived(documents.shown);
  const names = $derived(path === null ? [] : tags.tagsOf(path));
  const description = $derived(path === null ? "" : tags.descriptionOf(path));
  const marksOn = $derived(tags.overview?.marks === true);

  /** The file's tags with their marks (this file's project chain), when marks show. */
  let refs = $state.raw<{ path: string; tags: readonly TagRefView[] } | null>(null);
  $effect(() => {
    const at = path;
    // the index changes with every tag change (the marks may have changed too)
    void tags.index?.rev;
    if (at === null || !marksOn || names.length === 0) {
      refs = null;
      return;
    }
    let gone = false;
    void tags.fileMeta(at).then((meta) => {
      if (!gone && meta !== null) refs = { path: at, tags: meta.tags };
    });
    return () => {
      gone = true;
    };
  });

  function markOf(name: string): ReturnType<typeof tags.markOf> {
    if (refs === null || refs.path !== path) return null;
    const ref = refs.tags.find((r) => r.name === name);
    return ref ? tags.markOf(ref) : null;
  }

  function show(name: string, event: MouseEvent): void {
    tags.pick(name, event.ctrlKey || event.metaKey);
    ui.showSidebar(PANEL);
  }
</script>

{#if path !== null && (names.length > 0 || description !== "")}
  <div class="strip" role="group" aria-label={t("tags.label")}>
    {#each names as name (name)}
      {@const style = tags.styleOf(name)}
      {@const mark = markOf(name)}
      <TagChip
        {name}
        color={style?.color}
        icon={style?.icon}
        status={mark?.status ?? null}
        statusWords={mark?.words ?? ""}
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
