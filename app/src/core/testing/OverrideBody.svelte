<script lang="ts">
  import type { ViewerProps } from "../plugins/props";

  let { view }: ViewerProps = $props();
</script>

<div class="override-body">override of {view.path}</div>
