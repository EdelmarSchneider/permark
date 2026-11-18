<script lang="ts">
  // The Panels button of the status bar, beside the right panel's toggle and drawn like it: a menu of every view the
  // window offers, under its region, each on | off (`viewsMenu`). The palette's "Panels…" opens the same menu, over
  // this button. When the menu goes, the keyboard is back where it was, else (nothing had it) on this button.
  import PanelsTopLeft from "@lucide/svelte/icons/panels-top-left";
  import { onDestroy, tick } from "svelte";
  import { keyHint } from "../commands/hint";
  import ContextMenu from "../explorer/ContextMenu.svelte";
  import { t } from "../i18n";
  import { returnFocus } from "./sides";
  import { ui } from "./ui.svelte";
  import { viewsEntries } from "./viewsMenu.svelte";

  let button: HTMLButtonElement | undefined = $state();

  const hint = $derived(keyHint("panel.views"));
  const tip = $derived(
    hint === null ? t("panel.panelsHint") : `${t("panel.panelsHint")} (${hint})`,
  );

  function close(): void {
    ui.closePanels();
    void tick().then(() => returnFocus(button ?? null));
  }

  // the menu hangs on this button: gone with it (the window left the project), it must not open with the next one
  onDestroy(() => ui.closePanels());
</script>

<button
  bind:this={button}
  type="button"
  class="icon-btn panels"
  aria-label={t("panel.panels")}
  aria-haspopup="menu"
  aria-expanded={ui.panels}
  title={tip}
  onclick={() => ui.togglePanels()}
>
  <PanelsTopLeft size={14} strokeWidth={1.8} aria-hidden="true" />
</button>
{#if ui.panels && button}
  {@const box = button.getBoundingClientRect()}
  <ContextMenu
    x={box.left}
    y={box.top - 4}
    above
    entries={viewsEntries()}
    label={t("panel.panels")}
    anchor={button}
    {close}
  />
{/if}

<style>
  .panels {
    flex: none;
    /* a 24 px target: the status bar's full height */
    width: 24px;
    height: 24px;
  }
</style>
