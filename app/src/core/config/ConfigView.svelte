<script lang="ts">
  // The viewer of the config formats: the sheet, read only. A file that cannot be shown as a sheet (too large, not
  // valid for its syntax) is shown as text by the text viewer, not here.
  import type { ViewerProps } from "../plugins/props";
  import { chooseMode } from "../editing/choose";
  import ConfigSheet from "./ConfigSheet.svelte";
  import { Matches } from "./matches.svelte";

  let { view }: ViewerProps = $props();

  const sheet = $derived(view.body.kind === "sheet" ? view.body.sheet : null);
  const editable = $derived(view.editable.editable === "surgical");
  const matches = new Matches();

  // an ignore file counts what its patterns catch, once it is shown (again when the file changes)
  $effect(() => {
    if (sheet?.kind !== "ignore") return;
    matches.request(view.path, null, true);
    return () => matches.stop();
  });
</script>

{#if sheet}
  <ConfigSheet
    {sheet}
    editable={false}
    source={editable ? (line) => chooseMode("source", line) : null}
    matches={sheet.kind === "ignore" ? matches.counts : null}
    capped={matches.capped}
  />
{/if}
