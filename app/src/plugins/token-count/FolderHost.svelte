<script lang="ts">
  // There while the plugin is on (a status item that draws nothing in the bar): it keeps the window's folder count in
  // step with the project it shows, counts the folders the person opens when the totals are wanted, and holds the
  // dialogs of the count (the guard's question, the summary).
  import { explorerExpanded } from "../../core/explorer/current";
  import { workspaceRoot } from "../../core/plugins/client";
  import FolderGuard from "./FolderGuard.svelte";
  import FolderSummary from "./FolderSummary.svelte";
  import { folders } from "./folders.svelte";
  import { tokens } from "./store.svelte";

  // the project this window shows: its counted folders and its job
  $effect(() => {
    void folders.load(workspaceRoot());
  });

  // totals the person asked to see: folders that open are counted when they are small
  $effect(() => {
    if (!(tokens.state?.folderTotals ?? false)) {
      folders.stopAuto();
      return;
    }
    folders.folderOpened(explorerExpanded());
  });

  // the dialogs go with the plugin
  $effect(() => () => folders.closeSummary());
</script>

<FolderGuard />
<FolderSummary />
