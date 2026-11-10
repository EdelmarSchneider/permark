<script lang="ts">
  // Always there while the plugin is on (a status item that draws nothing in the bar): the language dialog over the
  // window while something asks for it (the palette, the translate menu, a first run). Made anew for every open: a
  // dialog starts from the languages the window holds then.
  import { languageDialog, type LanguageDialogStore } from "./dialog.svelte";
  import LanguageDialog from "./LanguageDialog.svelte";

  type Props = { dialog?: LanguageDialogStore };
  let { dialog = languageDialog }: Props = $props();

  // the dialog goes with the plugin
  $effect(() => () => dialog.close());
</script>

{#if dialog.state.status === "open"}
  <LanguageDialog {dialog} />
{/if}
