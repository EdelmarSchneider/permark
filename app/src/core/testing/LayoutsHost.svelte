<script lang="ts">
  // The window's regions with the Layouts dialog mounted the way the app mounts it: for the tests of the dialog's
  // flows (what opens it, what it changes in the regions, where the keyboard goes when it closes).
  import LayoutsDialog from "../shell/LayoutsDialog.svelte";
  import { ui } from "../shell/ui.svelte";
  import RegionsHost from "./RegionsHost.svelte";
</script>

<RegionsHost />
{#if ui.layoutsDialog.status === "open"}
  <LayoutsDialog />
{/if}
