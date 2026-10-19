<script lang="ts">
  import StructuredText from "../../core/doc/StructuredText.svelte";
  import type { ViewerProps } from "../../core/plugins/props";
  import { parseXml } from "./xml";

  let { view }: ViewerProps = $props();
  const text = $derived(view.body.kind === "text" ? view.body.text : "");
</script>

<StructuredText {text} parse={parseXml} />
