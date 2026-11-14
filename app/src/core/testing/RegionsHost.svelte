<script lang="ts">
  // The views of the window and its three regions, without the rest of the app: for the tests of where a view is
  // drawn and what a move keeps. No workspace is shown: Files and Search have their tabs and slots, no body.
  import ViewHosts from "../shell/ViewHosts.svelte";
  import ViewRegion from "../shell/ViewRegion.svelte";
  import { arrangement } from "../shell/arrangement.svelte";
  import { windowStore } from "../window.svelte";
</script>

<ViewHosts store={windowStore} shown={null} outline={null} />
<!-- the frame's body and its center column, as the frame names them -->
<div data-frame="body">
  <aside data-region="sidebar">
    {#if arrangement.drawn("left").length > 0}<ViewRegion region="left" />{/if}
  </aside>
  <div data-frame="center">
    <aside data-region="bottom">
      {#if arrangement.drawn("bottom").length > 0}<ViewRegion region="bottom" />{/if}
    </aside>
  </div>
  <aside data-region="right">
    {#if arrangement.drawn("right").length > 0}<ViewRegion region="right" />{/if}
  </aside>
</div>
