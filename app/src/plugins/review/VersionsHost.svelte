<script lang="ts">
  // Always there while the plugin is on (a status item that draws nothing in the bar): the list of earlier versions
  // over the window while it is open. One dialog per list asked for: its place in the rows and its "now" start anew.
  import { review } from "./store.svelte";
  import VersionsDialog from "./VersionsDialog.svelte";

  const list = $derived(review.versions);
</script>

{#if list.status === "open"}
  {#key list.path}
    <VersionsDialog {list} />
  {/key}
{/if}
