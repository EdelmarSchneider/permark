<script lang="ts">
  import type { EmbedProps } from "../plugins/props";

  let { source, theme, display, title, start, marked }: EmbedProps = $props();
</script>

<span
  class="embed-probe"
  data-forced={theme ?? "none"}
  data-display={String(display)}
  data-title={title ?? "none"}
  data-start={start ?? "none"}
  data-marked={marked ?? "none"}>{source}</span
>
