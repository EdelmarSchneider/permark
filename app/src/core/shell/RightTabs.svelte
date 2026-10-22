<script lang="ts">
  // The views of the right panel as tabs (Outline | Live changes), drawn only when there are two or more: the outline
  // of the document (when it has one) and the right panels plugins offer now. Selecting shows a view, never hides
  // the panel. One Tab stop: arrows move between the tabs, Enter | Space select.
  import ListTree from "@lucide/svelte/icons/list-tree";
  import type { Component } from "svelte";
  import { t } from "../i18n";
  import { activeRight, rightPanels } from "./panels.svelte";
  import { toggleOutlinePanel } from "./sides";
  import { ui } from "./ui.svelte";

  type Props = { outline: boolean };
  let { outline }: Props = $props();

  type View = {
    id: string;
    label: string;
    icon: Component<{ size?: number }> | undefined;
    badge: string | null;
  };

  const OUTLINE = "outline";

  const views = $derived.by((): View[] => [
    ...(outline ? [{ id: OUTLINE, label: t("outline.tab"), icon: ListTree, badge: null }] : []),
    ...rightPanels().map((panel) => ({
      id: panel.id,
      label: t(panel.label),
      icon: panel.icon,
      badge: panel.badge?.() ?? null,
    })),
  ]);
  const selected = $derived(activeRight() ?? (outline ? OUTLINE : null));
  /** Many tabs: the ones that are not open show their icon alone (the label is the tooltip). */
  const crowded = $derived(views.length > 2);
  /** The tab that takes the Tab stop (the selected one until arrows move the focus). */
  let focused = $state<string | null>(null);
  const stop = $derived(
    views.find((v) => v.id === focused)?.id ??
      views.find((v) => v.id === selected)?.id ??
      views[0]?.id ??
      null,
  );
  let strip: HTMLElement | undefined = $state();

  function select(id: string): void {
    if (id === selected) return;
    if (id === OUTLINE) toggleOutlinePanel();
    else ui.toggleRight(id);
  }

  function keydown(event: KeyboardEvent): void {
    const at = views.findIndex((v) => v.id === stop);
    const last = views.length - 1;
    let to: number;
    switch (event.key) {
      case "ArrowRight":
        to = at >= last ? 0 : at + 1;
        break;
      case "ArrowLeft":
        to = at <= 0 ? last : at - 1;
        break;
      case "Home":
        to = 0;
        break;
      case "End":
        to = last;
        break;
      default:
        return;
    }
    event.preventDefault();
    const id = views[to]?.id ?? null;
    focused = id;
    strip?.querySelector<HTMLElement>(`[data-view="${id ?? ""}"]`)?.focus();
  }
</script>

{#if views.length >= 2}
  <!-- svelte-ignore a11y_interactive_supports_focus -->
  <div
    class="tabs"
    class:crowded
    role="tablist"
    aria-label={t("panel.tabs")}
    bind:this={strip}
    onkeydown={keydown}
    onfocusout={(event) => {
      if (!(event.relatedTarget instanceof Node) || !strip?.contains(event.relatedTarget)) {
        focused = null;
      }
    }}
  >
    {#each views as view (view.id)}
      {@const iconOnly = view.icon !== undefined && crowded && selected !== view.id}
      <button
        type="button"
        role="tab"
        class="tab"
        class:icon-only={iconOnly}
        data-view={view.id}
        tabindex={stop === view.id ? 0 : -1}
        aria-selected={selected === view.id}
        aria-label={view.badge === null ? view.label : `${view.label} (${view.badge})`}
        title={view.label}
        onclick={() => select(view.id)}
      >
        {#if view.icon}
          {@const Icon = view.icon}
          <Icon size={14} />
        {/if}
        {#if !iconOnly}<span class="label">{view.label}</span>{/if}
        {#if view.badge !== null}<span class="badge" aria-hidden="true">{view.badge}</span>{/if}
      </button>
    {/each}
  </div>
{/if}

<style>
  .tabs {
    display: flex;
    flex: none;
    gap: 2px;
    padding: var(--sp-2) var(--sp-2) 0;
    overflow: hidden;
    border-bottom: 1px solid var(--border);
  }
  /* the open tab gives way before the others do: every tab stays reachable */
  .tab {
    flex: 0 1 auto;
    min-width: 0;
    display: inline-flex;
    align-items: center;
    gap: 6px;
    padding: 5px var(--sp-3);
    border: 0;
    border-bottom: 2px solid transparent;
    background: transparent;
    color: var(--muted);
    font: inherit;
    font-size: var(--fs-ui-sm);
    cursor: pointer;
  }
  .tab.icon-only {
    flex: none;
    padding-inline: var(--sp-2);
  }
  .crowded .tab {
    padding-inline: var(--sp-2);
  }
  .crowded .tab.icon-only {
    padding-inline: 5px;
  }
  .label {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab[aria-selected="true"] {
    border-bottom-color: var(--accent);
    color: var(--text);
    font-weight: 600;
  }
  .badge {
    flex: none;
    min-width: 16px;
    padding: 0 5px;
    border-radius: 999px;
    background: var(--accent);
    color: var(--accent-fg);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    line-height: 16px;
    text-align: center;
    font-variant-numeric: tabular-nums;
  }
</style>
