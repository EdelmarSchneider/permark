<script lang="ts">
  // Always there while the plugin is on (a status item that draws nothing in the bar): the page setup dialog over the
  // window when the person asks for a PDF or a printout.
  import SetupDialog from "./SetupDialog.svelte";
  import { exporter } from "./store.svelte";

  // the dialog and a run go with the plugin
  $effect(() => () => exporter.close());
</script>

<SetupDialog />
