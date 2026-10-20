<script lang="ts">
  // A search snippet: text with the words the query matched marked. Parts are data, never HTML.
  import type { SnippetPart } from "../../api";
  import Marked from "./Marked.svelte";

  type Props = { parts: readonly SnippetPart[] };
  let { parts }: Props = $props();
</script>

<Marked runs={parts} />
