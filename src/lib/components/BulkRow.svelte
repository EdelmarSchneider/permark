<script lang="ts">
  import { visibleBulkItems, type BulkEntry, type BulkItem } from "../feed";
  import { formatNumber, t, tn } from "../i18n";
  import { canOpenInPane } from "../fileKinds";
  import { basename, folderLabel } from "../paths";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { feed } from "../stores/feed.svelte";
  import { relativeTime } from "../time";
  import Icon from "./Icon.svelte";

  type Props = { entry: BulkEntry };
  let { entry }: Props = $props();

  const open = $derived(feed.expanded.has(entry.id));
  const all = $derived(feed.showAll.has(entry.id));
  const items = $derived(visibleBulkItems(entry, all));
  const title = $derived(
    entry.reason === "gitOperation"
      ? t("feed.bulk.git", { count: formatNumber(entry.count) })
      : entry.reason === "large"
        ? t("feed.bulk.large", { count: formatNumber(entry.count) })
        : t("feed.bulk.overflow", { count: formatNumber(entry.count) }),
  );
  const meta = $derived(
    [
      entry.reason === "gitOperation" ? t("feed.bulk.reasonGit") : t("feed.bulk.reasonLarge"),
      tn("feed.bulk.docs", entry.docCount),
      ...(entry.rescan ? [t("feed.bulk.resynced")] : []),
    ].join(" · "),
  );

  const LETTER = { created: "A", modified: "M", removed: "D", renamed: "R" } as const;

  function openItem(item: BulkItem): void {
    if (canOpenInPane(item.path) && item.kind !== "removed") void app.openChanged(item.path);
  }
</script>

<button
  type="button"
  class="feed-row"
  aria-expanded={open}
  aria-controls="bulk-{entry.id}"
  title={`${title} · ${meta}`}
  onclick={() => feed.toggleBulk(entry.id)}
>
  <Icon name="layers" class="kind kind--bulk" />
  <span>
    <span class="feed-row__name">{title}</span>
    <span class="feed-row__meta">{meta}</span>
  </span>
  <span class="feed-row__end">
    <time class="feed-row__time">{relativeTime(entry.atMs, clock.now)}</time>
    <Icon name="chevron-down" size="sm" class="twisty" />
  </span>
</button>
{#if open}
  <div class="bulk-body" id="bulk-{entry.id}">
    {#each items as item (item.path)}
      <button type="button" class="mini-row" onclick={() => openItem(item)}>
        <span class={["badge", `badge--${LETTER[item.kind]}`]}>{LETTER[item.kind]}</span>
        <span class="mini-name">{basename(item.path)}</span>
        <span>{folderLabel(item.path) || t("feed.root")}</span>
      </button>
    {/each}
    {#if !all && entry.items.length > items.length}
      <button type="button" class="link-btn" onclick={() => feed.showAll.add(entry.id)}>
        {t("feed.bulk.showAll", { count: formatNumber(entry.items.length) })}
      </button>
    {/if}
    {#if entry.count > entry.items.length}
      <div class="feed-note" style="padding-left:0">
        {t("feed.bulk.partial", {
          shown: formatNumber(entry.items.length),
          count: formatNumber(entry.count),
        })}
      </div>
    {/if}
  </div>
{/if}
