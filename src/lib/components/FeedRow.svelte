<script lang="ts">
  import type { FileEntry } from "../feed";
  import { t, tn } from "../i18n";
  import { basename } from "../paths";
  import { clock } from "../stores/clock.svelte";
  import { relativeTime } from "../time";
  import Icon, { type IconName } from "./Icon.svelte";

  type Props = { entry: FileEntry; current: boolean; onopen: () => void };
  let { entry, current, onopen }: Props = $props();

  const ICONS: Record<FileEntry["kind"], IconName> = {
    created: "file-plus",
    modified: "file-text",
    removed: "file-minus",
    renamed: "arrow-right-left",
  };

  const kindLabel = $derived(t(`feed.kind.${entry.kind}`));
  const folder = $derived(entry.folder === "" ? t("feed.root") : entry.folder);
</script>

<button
  type="button"
  class={["feed-row", current && "is-current", entry.kind === "removed" && "feed-row--removed"]}
  aria-current={current ? "true" : undefined}
  onclick={onopen}
>
  <Icon name={ICONS[entry.kind]} class={`kind kind--${entry.kind}`} />
  <span>
    <span class="feed-row__name"
      ><span class="sr-only">{kindLabel}: </span>{basename(entry.path)}</span
    >
    <span class="feed-row__meta">
      {#if entry.worktree}
        <span class="wt-label"><Icon name="fork" size="xs" />{entry.worktree}</span>
      {/if}
      <span>{folder}</span>
      {#if entry.kind === "renamed" && entry.from}
        <span>· {t("feed.renamedFrom", { name: basename(entry.from) })}</span>
      {:else if entry.kind === "removed"}
        <span>· {t("feed.removed")}</span>
      {:else if entry.saves > 1}
        <span>· {tn("feed.saves", entry.saves)}</span>
      {/if}
    </span>
  </span>
  <time class="feed-row__time" datetime={new Date(entry.atMs).toISOString()}>
    {relativeTime(entry.atMs, clock.now)}
  </time>
</button>
