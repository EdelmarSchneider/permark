<script lang="ts">
  import DiagramView from "../../core/doc/DiagramView.svelte";
  import type { EmbedProps } from "../../core/plugins/props";
  import { drawPlantuml } from "./engine";

  let { source }: EmbedProps = $props();
</script>

<DiagramView {source} draw={drawPlantuml} />
