<script lang="ts">
  // The detail card of a project node (hover 500 ms | menu "Project details"): name, category (+ its official | local |
  // unlisted mark where official lists exist), description, path, what plugins say about the folder ("3 unread · 2 open
  // comments": the project's own state), whether the agent sees it, and two ways on. Non-modal: stays while the
  // pointer or the focus is inside; Esc closes it (focus back to the tree when it had it). Never covers its row.
  import type { ProjectRow } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import { t, tn } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import { projects } from "./instance.svelte";
  import StatusMark from "./StatusMark.svelte";
  import { officialEntry, statusWords } from "./vocabulary";

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
  /** The card's facts: its description for screen readers (said when it opens, the answer before the buttons). */
  const FACTS_ID = "project-card-facts";
  const GAP = 8;

  let card: HTMLElement | undefined = $state();
  const agentOn = $derived(conditions.current.agent.status !== "off");
  /** Blocks of plugins that know about projects (git: branch, changes, last commit). */
  const facts = $derived(contributions.visible("projectFacts", conditions.current));
  /** Plugin words about the folder (unread count): their tooltips, which say it in full. A plugin with its own block
   *  says its part there (git's changed files). */
  const said = $derived.by(() => {
    const own = new Set(facts.map((f) => f.owner));
    return contributions.visible("treeBadge", conditions.current).flatMap((b) => {
      if (own.has(b.owner)) return [];
      const badge = b.badge(row.path, true);
      return badge === null || badge.text === "" ? [] : [badge.title ?? badge.text];
    });
  });

  /** Counts plugins keep about the project (open comments), after the badge titles (unread). */
  const counts = $derived([
    ...said,
    ...contributions.visible("projectCount", conditions.current).flatMap((c) => c.count(row) ?? []),
  ]);
  /** An official category wears the official list's color (the tag palette). */
  const categoryTint = $derived.by(() => {
    if (row.category === null || row.categoryStatus !== "official") return null;
    const color = officialEntry(projects.view, "category", row.category)?.color ?? null;
    // the backend sends only names of the tag palette (`--tag-<name>`)
    return color === null ? null : `var(--tag-${color})`;
  });

  // the card wants the details of its project while it shows
  $effect(() => {
    const uid = row.uid;
    projects.look("card", "detail", uid === null ? [] : [uid]);
    return () => projects.look("card", "detail", []);
  });

  /** Right of the row when there is room, else below | above it; inside the window by 8 px. */
  function place(el: HTMLElement): void {
    const width = Math.min(WIDTH, window.innerWidth - 2 * GAP);
    const height = el.getBoundingClientRect().height;
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
    el.style.left = `${left}px`;
    el.style.top = `${top}px`;
    el.style.width = `${width}px`;
  }

  $effect(() => {
    if (!card) return;
    const el = card;
    place(el);
    if (from === "menu") el.querySelector<HTMLElement>("button")?.focus();
    // a plugin's block that loads later makes it taller: it is placed again (never over its row)
    if (typeof ResizeObserver === "undefined") return;
    const observer = new ResizeObserver(() => place(el));
    observer.observe(el);
    return () => observer.disconnect();
  });

  /** How it opened, as a plain value for the event handlers: a browser that blurs the focused button while the
   *  closing card leaves the page (Chromium) must not read the props of a card already going (R3, `derived_inert`). */
  let fromMenu = false;
  let gone = false;
  $effect.pre(() => {
    fromMenu = from === "menu";
  });
  $effect(() => () => {
    gone = true;
  });

  // Esc closes it from anywhere while it shows
  $effect(() => {
    const onkey = (e: KeyboardEvent): void => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      e.stopPropagation();
      close(fromMenu || (card?.contains(document.activeElement) ?? false));
    };
    window.addEventListener("keydown", onkey, true);
    return () => window.removeEventListener("keydown", onkey, true);
  });

  function onfocusout(e: FocusEvent): void {
    // focus left the card for something else (Tab out, a click elsewhere): it goes; a card already closing (its own
    // button took it away) has nothing to do
    if (gone || projects.card === null) return;
    if (fromMenu && !(e.relatedTarget instanceof Node && card?.contains(e.relatedTarget))) {
      close(false);
    }
  }
</script>

<div
  bind:this={card}
  class="card"
  role="dialog"
  aria-label={t("projects.card.dialog", { name: row.name })}
  aria-describedby={FACTS_ID}
  tabindex="-1"
  onpointerenter={() => hovering(true)}
  onpointerleave={() => hovering(false)}
  {onfocusout}
>
  <div class="title">
    <span class="name">{row.name}</span>
    {#if row.category}<span
        class="chip"
        class:tinted={categoryTint !== null}
        style:--c={categoryTint}
        >{#if row.categoryStatus !== null}<StatusMark
            status={row.categoryStatus}
            words={statusWords(row.categoryStatus, "category", row.short)}
          />{/if}<span class="chip-text">{row.category}</span></span
      >{/if}
  </div>
  <div class="facts" id={FACTS_ID}>
    {#if row.description}<p class="description">{row.description}</p>{/if}
    <p class="path">{row.path}</p>
    {#each facts as fact (fact.id)}
      {#await fact.card() then block}<block.default {row} />{/await}
    {/each}
    {#if counts.length > 0}<p class="line">{counts.join(" · ")}</p>{/if}
    {#if agentOn}
      <p class="line">
        {t(row.agent === "off" ? "projects.card.agentNot" : "projects.card.agentSees")}
      </p>
    {/if}
    {#if row.inner !== null && row.inner > 0}
      <p class="line">{tn("projects.card.inner", row.inner)}</p>
    {/if}
  </div>
  <div class="buttons">
    <button type="button" class="btn btn--sm" onclick={openTab}>{t("projects.card.openTab")}</button
    >
    <!-- the same words as the node menu (one name per action) -->
    <button type="button" class="btn btn--sm" onclick={openSettings}
      >{t("projects.menu.settings")}</button
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
  .facts {
    display: flex;
    flex-direction: column;
    gap: 4px;
    min-width: 0;
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
    display: inline-flex;
    flex: none;
    align-items: center;
    gap: 3px;
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
  .chip.tinted {
    background: color-mix(in srgb, var(--c) 14%, transparent);
    color: var(--c);
  }
  .chip-text {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
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
