<script lang="ts">
  // One row for something done in the Explorer (a move, a rename, a delete...): the summary, and what it touched.
  import type { OperationEntry } from "../feed";
  import { t } from "../i18n";
  import { basename, folderLabel, isMarkdown } from "../paths";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { feed } from "../stores/feed.svelte";
  import { relativeTime } from "../time";
  import Icon, { type IconName } from "./Icon.svelte";

  type Props = { entry: OperationEntry };
  let { entry }: Props = $props();

  const ICONS: Record<OperationEntry["kind"], IconName> = {
    moved: "arrow-right-left",
    renamed: "pencil",
    deleted: "trash",
    folder: "folder-plus",
    undone: "undo",
  };

  const open = $derived(feed.expanded.has(entry.id));
  const expandable = $derived(entry.items.length > 0 || entry.notes.length > 0);

  function openItem(path: string): void {
    if (isMarkdown(path) && entry.kind !== "deleted") void app.openDoc(path);
  }
</script>

<button
  type="button"
  class="feed-row feed-row--op"
  aria-expanded={expandable ? open : undefined}
  aria-controls={expandable ? `op-${entry.id}` : undefined}
  disabled={!expandable}
  onclick={() => feed.toggleBulk(entry.id)}
>
  <Icon name={ICONS[entry.kind]} class="kind kind--renamed" />
  <span>
    <span class="feed-row__name">{entry.summary}</span>
    {#if entry.notes.length > 0}
      <span class="feed-row__meta">{entry.notes[0]}</span>
    {/if}
  </span>
  <span class="feed-row__end">
    <time class="feed-row__time">{relativeTime(entry.atMs, clock.now)}</time>
    {#if expandable}<Icon name="chevron-down" size="sm" class="twisty" />{/if}
  </span>
</button>
{#if open}
  <div class="bulk-body" id="op-{entry.id}">
    {#each entry.items as item (item.path)}
      <button type="button" class="mini-row" onclick={() => openItem(item.path)}>
        <span class="mini-name">{basename(item.path)}</span>
        <span>
          {#if item.from}{folderLabel(item.from) || t("feed.root")} →
          {/if}{folderLabel(item.path) || t("feed.root")}
        </span>
      </button>
    {/each}
    {#each entry.notes as note, n (n)}
      <div class="feed-note" style="padding-left:0">{note}</div>
    {/each}
  </div>
{/if}
