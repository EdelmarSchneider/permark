<script lang="ts">
  // Status bar items plugins contribute, one slot of the bar (left | center | right), lowest `order` first. A plugin
  // off: its item is gone. The core's own item (live updates off) leads the left slot.
  import { conditions } from "../conditions/context.svelte";
  import { contributions } from "../plugins/instance.svelte";
  import LiveOff from "./LiveOff.svelte";

  type Props = { slot: "left" | "center" | "right" };
  let { slot }: Props = $props();

  const items = $derived(
    contributions
      .visible("statusItem", conditions.current)
      .filter((item) => item.slot === slot)
      .sort((a, b) => a.order - b.order),
  );
</script>

{#if slot === "left"}<LiveOff />{/if}
{#each items as item (item.owner + item.id)}
  {#await item.view() then loaded}
    {@const View = loaded.default}
    <View />
  {/await}
{/each}
