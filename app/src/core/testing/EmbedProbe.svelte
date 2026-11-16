<script lang="ts">
  import type { EmbedProps } from "../plugins/props";

  let { source, theme, display }: EmbedProps = $props();
</script>

<span class="embed-probe" data-forced={theme ?? "none"} data-display={String(display)}
  >{source}</span
>
