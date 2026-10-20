<script lang="ts">
  // Always there while the plugin is on (a status item that draws nothing in the bar): opens the tag editor over the
  // window when something asks for it (the key T, the header pencil, the Explorer menu).
  import TagEditor from "./TagEditor.svelte";
  import { tags } from "./store.svelte";

  const editing = $derived(tags.editing);
</script>

{#if editing}
  {#key editing.path}
    <TagEditor path={editing.path} />
  {/key}
{/if}
