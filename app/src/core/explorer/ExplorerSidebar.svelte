<script lang="ts">
  // The Explorer of the project this window shows: one store per project shown (the parent re-creates this component
  // when the workspace changes), listening while it lives.
  import { untrack } from "svelte";
  import { api, type WorkspaceId } from "../../api";
  import type { WindowStore } from "../window.svelte";
  import { setExplorer } from "./current";
  import ExplorerPanel from "./ExplorerPanel.svelte";
  import { ui } from "../shell/ui.svelte";
  import { ExplorerStore } from "./store.svelte";

  type Props = {
    store: WindowStore;
    /** The project this Explorer is for (the parent makes a new one for each). */
    workspace: WorkspaceId;
    document: string | null;
  };
  let { store, workspace, document: active }: Props = $props();

  // read once: the Explorer keeps naming the project it was made for, also in calls that land after a switch
  const made = untrack(() => workspace);
  const explorer = new ExplorerStore(api, made, {
    fail: (error) => store.notifyFileOpFailed(error),
    // one file-operation note with Undo at a time: its Undo is always the newest operation, the one Ctrl+Z takes back
    note: (text, ...actions) =>
      actions.length === 0 ? store.notifyNote(text) : store.notifyKeyed("fileOp", text, ...actions),
    view: () => store.label,
    linkUpdate: () => store.linkUpdate,
    openProject: (path) => void store.open(path),
    referenceTemplate: () => store.referenceTemplate,
    initProject: (path, name) => ui.askInit(path, name),
  });

  // listens while shown; closed at once when the component goes, also while a big list still loads (a teardown that
  // waited for it would close the list of the folder the tab shows next)
  $effect(() => {
    void explorer.start();
    return () => explorer.close();
  });

  // the rest of the window can point at this tree while it lives
  $effect(() => {
    setExplorer(explorer);
    return () => setExplorer(null);
  });

  // the open document stays in the list and comes into view
  $effect(() => {
    void explorer.setActive(active);
  });
</script>

<ExplorerPanel store={explorer} />
