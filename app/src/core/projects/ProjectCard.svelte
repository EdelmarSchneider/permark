<script lang="ts">
  // The detail card of a project node (hover 500 ms | menu "Project details"): name, category, description, path,
  // what plugins say about the folder (unread), whether the agent sees it, and two ways on. Non-modal: stays while the
  // pointer or the focus is inside; Esc closes it (focus back to the tree when it had it). Never covers its row.
  import type { ProjectRow } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import { t, tn } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";

  type Props = {
    row: ProjectRow;
    anchor: DOMRect;
    /** `menu`: focus goes to the first button. */
    from: "hover" | "menu";
    close: (refocus: boolean) => void;
    openTab: () => void;
    openSettings: () => void;
    /** The pointer came in | went out (the hover timer of the Explorer keeps it open while inside). */
    hovering: (inside: boolean) => void;
  };
  let { row, anchor, from, close, openTab, openSettings, hovering }: Props = $props();

  const WIDTH = 320;
  const GAP = 8;

  let card: HTMLElement | undefined = $state();
  const agentOn = $derived(conditions.current.agent.status !== "off");
  /** Plugin words about the folder (unread count, changed files): their tooltips, which say it in full. */
  const said = $derived(
    contributions.visible("treeBadge", conditions.current).flatMap((b) => {
      const badge = b.badge(row.path, true);
      return badge === null || badge.text === "" ? [] : [badge.title ?? badge.text];
    }),
  );

  // placed right of the row when there is room, else below | above it; inside the window by 8 px
  $effect(() => {
    if (!card) return;
    const width = Math.min(WIDTH, window.innerWidth - 2 * GAP);
    const height = card.getBoundingClientRect().height;
    let left = anchor.right + GAP;
    let top = anchor.top;
    if (left + width > window.innerWidth - GAP) {
      left = Math.max(GAP, Math.min(anchor.left + 24, window.innerWidth - width - GAP));
      top =
        anchor.bottom + GAP + height <= window.innerHeight - GAP
          ? anchor.bottom + 4
          : anchor.top - height - 4;
    }
    top = Math.max(GAP, Math.min(top, window.innerHeight - height - GAP));
    card.style.left = `${left}px`;
    card.style.top = `${top}px`;
    card.style.width = `${width}px`;
    if (from === "menu") card.querySelector<HTMLElement>("button")?.focus();
  });

  // Esc closes it from anywhere while it shows
  $effect(() => {
    const onkey = (e: KeyboardEvent): void => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      e.stopPropagation();
      close(from === "menu" || (card?.contains(document.activeElement) ?? false));
    };
    window.addEventListener("keydown", onkey, true);
    return () => window.removeEventListener("keydown", onkey, true);
  });

  function onfocusout(e: FocusEvent): void {
    // focus left the card for something else (Tab out, a click elsewhere): it goes
    if (from === "menu" && !(e.relatedTarget instanceof Node && card?.contains(e.relatedTarget))) {
      close(false);
    }
  }
</script>

<div
  bind:this={card}
  class="card"
  role="dialog"
  aria-label={t("projects.card.dialog", { name: row.name })}
  tabindex="-1"
  onpointerenter={() => hovering(true)}
  onpointerleave={() => hovering(false)}
  {onfocusout}
>
  <div class="title">
    <span class="name">{row.name}</span>
    {#if row.category}<span class="chip">{row.category}</span>{/if}
  </div>
  {#if row.description}<p class="description">{row.description}</p>{/if}
  <p class="path">{row.path}</p>
  {#if said.length > 0}<p class="line">{said.join(" · ")}</p>{/if}
  {#if agentOn}
    <p class="line">
      {t(row.agent === "off" ? "projects.card.agentNot" : "projects.card.agentSees")}
    </p>
  {/if}
  {#if row.inner !== null && row.inner > 0}
    <p class="line">{tn("projects.card.inner", row.inner)}</p>
  {/if}
  <div class="buttons">
    <button type="button" class="btn btn--sm" onclick={openTab}>{t("projects.card.openTab")}</button
    >
    <button type="button" class="btn btn--sm" onclick={openSettings}
      >{t("projects.card.settings")}</button
    >
  </div>
</div>

<style>
  .card {
    position: fixed;
    z-index: 46;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
    color: var(--text);
    font-size: var(--fs-ui-sm);
    outline: none;
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }
  .name {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
    font-weight: 600;
  }
  .chip {
    flex: none;
    max-width: 50%;
    overflow: hidden;
    padding: 0 6px;
    border-radius: 999px;
    background: var(--surface-2);
    color: var(--muted);
    font-size: var(--fs-ui-xs);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .path,
  .line {
    color: var(--muted);
  }
  .path {
    font-size: var(--fs-ui-xs);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    margin-top: var(--sp-2);
  }
  @media (prefers-reduced-motion: reduce) {
    .card {
      animation: none;
    }
  }
</style>
