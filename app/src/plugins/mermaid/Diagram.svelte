<script lang="ts">
  import DiagramView from "../../core/doc/DiagramView.svelte";
  import type { EmbedProps } from "../../core/plugins/props";
  import { drawMermaid } from "./engine";

  let { source }: EmbedProps = $props();
</script>

<DiagramView {source} draw={drawMermaid} />
