<script lang="ts">
  import { t } from "../i18n";
  import { ui } from "../stores/ui.svelte";
</script>

{#if ui.drag}
  <div class="drop-overlay" role="status">
    {ui.drag.count === 1 ? t("welcome.dropActive") : t("welcome.dropOne")}
  </div>
{/if}
