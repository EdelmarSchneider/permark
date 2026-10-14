<script lang="ts">
  import type { Snippet } from "svelte";
  import Icon, { type IconName } from "./Icon.svelte";

  type Props = { icon: IconName; title: string; text?: string; children?: Snippet };
  let { icon, title, text, children }: Props = $props();
</script>

<div class="notice" role="status">
  <Icon name={icon} />
  <h2 class="notice__title">{title}</h2>
  {#if text}<p class="notice__text">{text}</p>{/if}
  {#if children}<div class="notice__actions">{@render children()}</div>{/if}
</div>
