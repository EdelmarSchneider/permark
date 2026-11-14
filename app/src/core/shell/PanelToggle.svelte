<script lang="ts">
  // Show | hide a region, in the status bar: the sidebar's first on the left, the bottom panel's right after it, the
  // right panel's last on the right. Pressed = the region is on screen. Shown only where the toggle applies (App).
  // The bottom panel's cannot act while the window is too short to draw the panel: it stays in reach of the keyboard
  // and says what to do instead.
  import PanelBottom from "@lucide/svelte/icons/panel-bottom";
  import PanelLeft from "@lucide/svelte/icons/panel-left";
  import PanelRight from "@lucide/svelte/icons/panel-right";
  import { keyHint } from "../commands/hint";
  import { t } from "../i18n";
  import { regionShown } from "./layout";
  import { bottomSqueezed, regionState, toggleRegion } from "./sides";

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
  /** Why a press does nothing now (`null`: it acts). */
  const blocked = $derived(
    of.region === "bottom" && bottomSqueezed() ? t("panel.bottomTooShort") : null,
  );
  const whyId = $props.id();
</script>

<!-- `aria-disabled`, not `disabled`: the keyboard still reaches it and hears why -->
<button
  type="button"
  class="icon-btn toggle"
  aria-label={t(of.name)}
  aria-pressed={shown}
  aria-disabled={blocked === null ? undefined : "true"}
  aria-describedby={blocked === null ? undefined : whyId}
  title={blocked ?? (hint === null ? action : `${action} (${hint})`)}
  onclick={() => toggleRegion(of.region)}
>
  <Icon size={14} strokeWidth={1.9} aria-hidden="true" />
</button>
{#if blocked !== null}
  <span class="sr-only" id={whyId}>{blocked}</span>
{/if}

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
  /* cannot act now: the look of a disabled icon button, still a Tab stop */
  .toggle[aria-disabled="true"],
  .toggle[aria-disabled="true"]:hover {
    background: transparent;
    color: var(--muted);
    opacity: 0.4;
    cursor: default;
  }
</style>
