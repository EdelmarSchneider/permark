<script lang="ts">
  // Status bar items plugins contribute (word count, ...), by order. A plugin off: its item is gone.
  import { conditions } from "../conditions/context.svelte";
  import { contributions } from "../plugins/instance.svelte";

  const items = $derived(
    [...contributions.visible("statusItem", conditions.current)].sort((a, b) => a.order - b.order),
  );
</script>

{#each items as item (item.owner + item.id)}
  {#await item.view() then loaded}
    {@const View = loaded.default}
    <View />
  {/await}
{/each}
