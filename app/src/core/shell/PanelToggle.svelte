<script lang="ts">
  // Show | hide a side panel, at its end of the status bar: the sidebar's first on the left, the right panel's last on
  // the right. Pressed = the panel is on screen. Shown only where the toggle applies (App).
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import { keyHint } from "../commands/hint";
  import { t } from "../i18n";
  import { rightShown } from "./layout";
  import { layout } from "./layout.svelte";
  import { rightSide, toggleRightPanel, toggleSidebar } from "./sides";

  type Props = { side: "sidebar" | "right" };
  let { side }: Props = $props();

  const shown = $derived(side === "sidebar" ? !layout.sidebarHidden : rightShown(rightSide()));
  const hint = $derived(keyHint(side === "sidebar" ? "sidebar.toggle" : "panel.toggle"));
  const action = $derived.by(() => {
    if (side === "sidebar") return shown ? t("panel.hideSidebar") : t("panel.showSidebar");
    return shown ? t("panel.hideRight") : t("panel.showRight");
  });
</script>

<button
  type="button"
  class="icon-btn toggle"
  aria-label={side === "sidebar" ? t("panel.sidebar") : t("panel.right")}
  aria-pressed={shown}
  title={hint === null ? action : `${action} (${hint})`}
  onclick={() => (side === "sidebar" ? toggleSidebar() : toggleRightPanel())}
>
  {#if side === "sidebar"}
    <PanelLeft size={14} strokeWidth={1.9} aria-hidden="true" />
  {:else}
    <PanelRight size={14} strokeWidth={1.9} aria-hidden="true" />
  {/if}
</button>

<style>
  .toggle {
    flex: none;
    width: 22px;
    height: 22px;
  }
  /* on screen is the usual state: quiet, no accent (the panel itself says it is there) */
  .toggle[aria-pressed="true"] {
    background: transparent;
    color: var(--muted);
  }
  .toggle[aria-pressed="true"]:hover {
    background: var(--hover);
    color: var(--text);
  }
</style>
