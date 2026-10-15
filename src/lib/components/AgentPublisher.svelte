<script lang="ts">
  // Keeps the agent connection's message up to date: whatever it depends on changing (the document, the mode, the
  // section on screen, unsaved edits, the selection) sends it again a moment later. Draws nothing.
  import { agent } from "../stores/agent.svelte";

  $effect(() => {
    agent.track();
  });
</script>

<!-- the selection is not state of the app: the document says when it changes -->
<svelte:document onselectionchange={() => agent.schedule()} />
