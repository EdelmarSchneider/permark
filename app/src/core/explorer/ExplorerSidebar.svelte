<script lang="ts">
  // The Explorer of the project this window shows: one store per project shown (the parent re-creates this component
  // when the workspace changes), listening while it lives.
  import { api } from "../../api";
  import type { WindowStore } from "../window.svelte";
  import { setExplorer } from "./current";
  import ExplorerPanel from "./ExplorerPanel.svelte";
  import { ui } from "../shell/ui.svelte";
  import { ExplorerStore } from "./store.svelte";

  type Props = { store: WindowStore; document: string | null };
  let { store, document: active }: Props = $props();

  const explorer = new ExplorerStore(api, {
    fail: (error) => store.notifyFileOpFailed(error),
    note: (text) => store.notifyNote(text),
    linkUpdate: () => store.linkUpdate,
    openProject: (path) => void store.open(path),
    referenceTemplate: () => store.referenceTemplate,
    initProject: (path, name) => ui.askInit(path, name),
  });

  // listens while shown (teardown even when the component goes before the first load ends)
  $effect(() => {
    let off: (() => void) | null = null;
    let gone = false;
    void explorer.start().then((stop) => {
      if (gone) stop();
      else off = stop;
    });
    return () => {
      gone = true;
      off?.();
    };
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
