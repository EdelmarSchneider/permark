<script lang="ts">
  // Show | hide a region, in the status bar: the sidebar's first on the left, the bottom panel's right after it, the
  // right panel's last on the right. Pressed = the region is on screen. Shown only where the toggle applies (App).
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import { keyHint } from "../commands/hint";
  import { t } from "../i18n";
  import { regionShown } from "./layout";
  import { regionState, toggleRegion } from "./sides";

  type Props = { side: "sidebar" | "right" | "bottom" };
  let { side }: Props = $props();

  /** Per toggle: its region, its command (the key in the tooltip), its words, its icon. */
  const SIDES = {
    sidebar: {
      region: "left",
      command: "sidebar.toggle",
      name: "panel.sidebar",
      show: "panel.showSidebar",
      hide: "panel.hideSidebar",
      icon: PanelLeft,
    },
    right: {
      region: "right",
      command: "panel.toggle",
      name: "panel.right",
      show: "panel.showRight",
      hide: "panel.hideRight",
      icon: PanelRight,
    },
    bottom: {
      region: "bottom",
      command: "panel.toggleBottom",
      name: "panel.bottom",
      show: "panel.showBottom",
      hide: "panel.hideBottom",
      icon: PanelBottom,
    },
  } as const;

  const of = $derived(SIDES[side]);
  const shown = $derived(regionShown(regionState(of.region)));
  const hint = $derived(keyHint(of.command));
  const action = $derived(t(shown ? of.hide : of.show));
  const Icon = $derived(of.icon);
</script>

<button
  type="button"
  class="icon-btn toggle"
  aria-label={t(of.name)}
  aria-pressed={shown}
  title={hint === null ? action : `${action} (${hint})`}
  onclick={() => toggleRegion(of.region)}
>
  <Icon size={14} strokeWidth={1.9} aria-hidden="true" />
</button>

<style>
  .toggle {
    flex: none;
    /* a 24 px target: the status bar's full height */
    width: 24px;
    height: 24px;
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
