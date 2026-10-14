<script lang="ts">
  import { t, tn } from "../i18n";
  import { app } from "../stores/app.svelte";
  import { docs } from "../stores/docs.svelte";
  import { feed } from "../stores/feed.svelte";
  import BulkRow from "./BulkRow.svelte";
  import FeedRow from "./FeedRow.svelte";
  import OpRow from "./OpRow.svelte";

  const view = $derived(feed.view);
</script>

{#if view.entries.length === 0}
  <p class="panel-note">{t("feed.empty")}</p>
{:else}
  <ul class="feed">
    {#each view.entries as entry (entry.id)}
      <li>
        {#if entry.type === "bulk"}
          <BulkRow {entry} />
        {:else if entry.type === "op"}
          <OpRow {entry} />
        {:else}
          <FeedRow
            {entry}
            current={docs.path === entry.path}
            onopen={() => void app.openChanged(entry.path)}
          />
        {/if}
      </li>
    {/each}
  </ul>
{/if}
{#if view.hidden > 0}
  <div class="feed-note">{tn("feed.hidden", view.hidden)}</div>
{/if}
