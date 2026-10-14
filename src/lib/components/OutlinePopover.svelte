<script lang="ts">
  import { onMount, untrack } from "svelte";
  import { t } from "../i18n";
  import { ui } from "../stores/ui.svelte";
  import OutlineList from "./OutlineList.svelte";
  import Popover from "./Popover.svelte";

  // Simple mode has no right panel: the same list opens under the document header (Ctrl+Alt+O).
  let list: OutlineList | undefined = $state();

  function headerAnchor(): { left: number; right: number; top: number; bottom: number } {
    const box = document.querySelector<HTMLElement>(".doc-header")?.getBoundingClientRect();
    if (!box) return { left: 8, right: 8, top: 0, bottom: 44 };
    return { left: box.right - 8, right: box.right - 8, top: box.top, bottom: box.bottom };
  }
  const anchor = untrack(headerAnchor);

  function close(): void {
    ui.outlinePopover = false;
  }

  onMount(() => {
    // the popover is positioned in an effect; focus the list once it is in place
    const timer = setTimeout(() => list?.focus(), 0);
    return () => clearTimeout(timer);
  });
</script>

<Popover {anchor} align="end" variant="panel" wide label={t("outline.label")} onclose={close}>
  <OutlineList bind:this={list} idPrefix="outline-popover" onpick={close} />
</Popover>
