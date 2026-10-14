<script lang="ts">
  import { vault } from "../stores/vault.svelte";
  import { session } from "../stores/session.svelte";
  import { ui } from "../stores/ui.svelte";
  import ActivityBar from "./ActivityBar.svelte";
  import MainPane from "./MainPane.svelte";
  import RightPanel from "./RightPanel.svelte";
  import Sidebar from "./Sidebar.svelte";
  import StatusBar from "./StatusBar.svelte";
</script>

{#if vault.isSimple}
  <div class="app app--simple">
    <MainPane />
  </div>
{:else}
  <div
    class={["app", ui.resizing && "is-resizing"]}
    data-sidebar={session.left.open ? "shown" : "hidden"}
    data-right={session.right.open ? "shown" : "hidden"}
    style:--sb="{session.left.open ? session.left.width : 0}px"
    style:--rp="{session.right.open ? session.right.width : 0}px"
  >
    <ActivityBar />
    <Sidebar />
    <MainPane />
    <RightPanel />
    <StatusBar />
  </div>
{/if}
