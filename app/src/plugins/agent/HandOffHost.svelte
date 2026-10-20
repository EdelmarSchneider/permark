<script lang="ts">
  // There while the plugin is on in a project (a status item that draws nothing in the bar): the hand-off dialog over
  // the window when the command or the status popover opens it.
  import HandOffDialog from "./HandOffDialog.svelte";
  import { handOff } from "./handoff.svelte";

  // the dialog goes with the plugin
  $effect(() => () => handOff.close());
</script>

<HandOffDialog />
