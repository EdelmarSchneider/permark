<script lang="ts">
  import { onMount } from "svelte";
  import { t } from "../i18n";
  import { registerOverlay } from "../overlays";
  import { ui } from "../stores/ui.svelte";

  type Props = { src: string };
  let { src }: Props = $props();

  onMount(() => registerOverlay(() => (ui.zoomImage = null)));
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="zoom-overlay" onclick={() => (ui.zoomImage = null)}>
  <img {src} alt={t("doc.imageZoomed")} />
</div>
