<script lang="ts">
  // The viewer of the config formats: the sheet, read only. A file that cannot be shown as a sheet (too large, not
  // valid for its syntax) is shown as text by the text viewer, not here. A line asked for (a code reference) brings
  // the row holding it into view, tinted a moment (rows of tables stay where they are: the sheet opens at its top).
  import { untrack } from "svelte";
  import type { ViewerProps } from "../plugins/props";
  import { documents } from "../doc/document.svelte";
  import { chooseMode } from "../editing/choose";
  import ConfigSheet from "./ConfigSheet.svelte";
  import { Matches } from "./matches.svelte";

  let { view }: ViewerProps = $props();

  const sheet = $derived(view.body.kind === "sheet" ? view.body.sheet : null);
  const editable = $derived(view.editable.editable === "surgical");
  const matches = new Matches();
  let shown = $state<{ locate(line: number, asked: boolean): Promise<void> }>();
  let handled = 0;

  // an ignore file counts what its patterns catch, once it is shown (again when the file changes)
  $effect(() => {
    if (sheet?.kind !== "ignore") return;
    matches.request(view.path, null, true);
    return () => matches.stop();
  });

  $effect(() => {
    const request = documents.lineRequest;
    const target = shown;
    if (!request || !target || request.path !== view.path || request.seq === handled) return;
    handled = request.seq;
    untrack(() => void target.locate(request.line, true));
  });
</script>

{#if sheet}
  <ConfigSheet
    bind:this={shown}
    {sheet}
    editable={false}
    source={editable ? (line) => chooseMode("source", line) : null}
    matches={sheet.kind === "ignore" ? matches.counts : null}
    capped={matches.capped}
    spread
  />
{/if}
