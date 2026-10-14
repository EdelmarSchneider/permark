<script lang="ts">
  import { edit } from "../stores/edit.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import Banners from "./Banners.svelte";
  import DocHeader from "./DocHeader.svelte";
  import DocView from "./DocView.svelte";
  import EditBars from "./EditBars.svelte";
  import FindBar from "./FindBar.svelte";
  import SourceEditor from "./SourceEditor.svelte";
  import Toast from "./Toast.svelte";
</script>

<main class="main" data-width={session.width}>
  <DocHeader />
  <Banners />
  {#if edit.active}<EditBars />{/if}
  <DocView />
  {#if edit.active}
    <!-- one editor per document: opening another document starts a fresh one -->
    {#key edit.path}<SourceEditor />{/key}
  {/if}
  {#if ui.find.open && !edit.active}<FindBar />{/if}
  <Toast />
</main>
