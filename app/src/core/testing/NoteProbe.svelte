<script lang="ts">
  // A plugin's strip under the Explorer list, for tests: a region with one button that hands focus back to the list.
  import type { ExplorerNoteProps } from "../plugins/registry.svelte";

  let { focusList }: ExplorerNoteProps = $props();
</script>

<div class="probe-note" role="region" aria-label="probe note">
  <button type="button" onclick={focusList}>Done</button>
</div>
