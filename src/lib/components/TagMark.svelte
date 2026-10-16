<script lang="ts">
  import type { TagStyle } from "../tagStyle";
  import Icon from "./Icon.svelte";

  // The small mark of a tag in a list: its icon in its color, or a dot of its color when it has no icon.
  type Props = { style: TagStyle };
  let { style }: Props = $props();
</script>

<span class="tag-mark" data-tag-color={style.color ?? undefined} aria-hidden="true">
  {#if style.icon}
    <Icon name={style.icon} size="sm" />
  {:else}
    <span class="tag-mark__dot"></span>
  {/if}
</span>
