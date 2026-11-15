<script lang="ts">
  import type { EmbedProps } from "../plugins/props";

  let { source, theme }: EmbedProps = $props();
</script>

<span class="embed-probe" data-forced={theme ?? "none"}>{source}</span>
