<script lang="ts">
  import type { ViewerProps } from "../plugins/props";

  let { view }: ViewerProps = $props();
</script>

<div class="plain-body" data-path={view.path}>plain</div>
