<script lang="ts">
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import Banners from "./Banners.svelte";
  import DocHeader from "./DocHeader.svelte";
  import DocView from "./DocView.svelte";
  import FindBar from "./FindBar.svelte";
  import Toast from "./Toast.svelte";
</script>

<main class={["main", session.wide && "is-wide"]}>
  <DocHeader />
  <Banners />
  <DocView />
  {#if ui.find.open}<FindBar />{/if}
  <Toast />
</main>
