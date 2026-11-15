<script lang="ts">
  import DiagramView from "../../core/doc/DiagramView.svelte";
  import type { EmbedProps } from "../../core/plugins/props";
  import { drawPlantuml } from "./engine";

  let { source, theme }: EmbedProps = $props();
</script>

<DiagramView {source} {theme} draw={drawPlantuml} />
