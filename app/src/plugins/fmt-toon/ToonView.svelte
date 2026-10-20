<script lang="ts">
  // A TOON file: its cards and tables (the config sheet), with "Copy as JSON" above them.
  import { api } from "../../api";
  import ConfigView from "../../core/config/ConfigView.svelte";
  import type { ViewerProps } from "../../core/plugins/props";
  import CopyJson from "./CopyJson.svelte";

  let { view, workspace }: ViewerProps = $props();

  async function text(): Promise<string | null> {
    const result = await api.documentText(view.path);
    return result.ok ? result.value : null;
  }
</script>

<div class="toon">
  <div class="bar"><CopyJson {text} /></div>
  <ConfigView {view} {workspace} />
</div>

<style>
  .bar {
    display: flex;
    justify-content: flex-end;
    width: min(100%, var(--measure-wide));
    margin: 0 auto;
    padding: var(--sp-3) var(--sp-5) 0;
  }
</style>
