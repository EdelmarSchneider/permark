<script lang="ts">
  // A search snippet: text with the words the query matched marked. Parts are data, never HTML.
  import type { SnippetPart } from "../../api";

  type Props = { parts: readonly SnippetPart[] };
  let { parts }: Props = $props();
</script>

{#each parts as part, i (i)}{#if part.mark}<mark>{part.text}</mark>{:else}{part.text}{/if}{/each}

<style>
  mark {
    background: var(--mark);
    color: inherit;
    border-radius: 2px;
  }
</style>
