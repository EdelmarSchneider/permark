<script lang="ts">
  // Always there while the plugin is on (a status item that draws nothing in the bar): the converter dialog over the
  // window when something opens it (the Explorer menu, the palette, a code block, a table, a selection).
  import { documents } from "../../core/doc/document.svelte";
  import ConvertDialog from "./ConvertDialog.svelte";
  import { converter } from "./store.svelte";

  converter.bind((path) => documents.navigate(path, null));

  // the dialog goes with the plugin
  $effect(() => () => converter.close());
</script>

<ConvertDialog />
