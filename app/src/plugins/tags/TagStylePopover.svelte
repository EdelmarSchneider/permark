<script lang="ts">
  // The look of one tag, and where it stands. On top (where official lists exist): its mark + words, then "Make
  // official in {root}" (local | unlisted; asks first) or, for an official tag, "Edit the official list…" (Settings >
  // Projects inside at that name). Below: a color from the palette and an icon ("none" takes it away), applied as
  // picked; an official tag whose look the official list sets says so instead. Focus goes to its first control when it
  // opens; Escape closes it (focus back on its opener); a click elsewhere closes it.
  import { onMount, tick, untrack } from "svelte";
  import type { TagStatus } from "../../api";
  import { t } from "../../core/i18n";
  import LookPicker from "../../core/looks/LookPicker.svelte";
  import { isColor, isIcon } from "../../core/looks/model";
  import PromoteAsk from "../../core/projects/PromoteAsk.svelte";
  import { projects } from "../../core/projects/instance.svelte";
  import StatusMark from "../../core/projects/StatusMark.svelte";
  import { officialEntry } from "../../core/projects/vocabulary";
  import { tags } from "./store.svelte";

  type Props = {
    tag: string;
    /** Goes away; `refocus`: focus back on the opener (Escape, a key path), not on a click elsewhere. */
    close: (refocus: boolean) => void;
    status?: TagStatus | null;
    /** Its mark's words (said with the status on top). */
    statusWords?: string;
    /** The button that opened it: a click there toggles it (not "a click elsewhere"). */
    opener?: HTMLElement | null;
  };
  let { tag, close, status = null, statusWords = "", opener = null }: Props = $props();

  let root: HTMLElement | undefined = $state();
  const style = $derived(tags.styleOf(tag));
  const color = $derived(isColor(style?.color) ? style.color : null);
  const icon = $derived(isIcon(style?.icon) ? style.icon : null);
  /** The official list sets this tag's look: the app's own look would not show. */
  const fixed = $derived.by(() => {
    if (status !== "official") return false;
    const entry = officialEntry(projects.view, "tag", tag);
    return entry !== null && (entry.color !== null || entry.icon !== null);
  });
  const promote = $derived(status !== "official" && tags.promotable);

  /** Its first control (the dialog itself when it has none). */
  function focusFirst(): void {
    const first = root?.querySelector<HTMLElement>("button:not(:disabled), a[href], input");
    (first ?? root)?.focus();
  }

  onMount(() => {
    focusFirst();
    const outside = (event: PointerEvent): void => {
      if (!(event.target instanceof Node) || !root || root.contains(event.target)) return;
      if (opener?.contains(event.target)) return;
      close(false);
    };
    window.addEventListener("pointerdown", outside, true);
    return () => window.removeEventListener("pointerdown", outside, true);
  });

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && !event.isComposing) {
      event.preventDefault();
      event.stopPropagation();
      close(true);
    }
  }

  /** Made official: the top now says so; focus on what is there now. */
  async function promoted(): Promise<void> {
    await tick();
    focusFirst();
  }

  // the status arrives after the promote (the tags view reloads): the control holding the focus may go with it ->
  // focus on what is there now, never the page
  $effect.pre(() => {
    void status;
    void fixed;
    untrack(() => {
      const held = document.activeElement;
      if (!root || !(held instanceof Node) || !root.contains(held)) return;
      void tick().then(() => {
        const now = document.activeElement;
        if (now === null || now === document.body || !now.isConnected) focusFirst();
      });
    });
  });

  function editList(): void {
    close(false);
    projects.showVocabulary("tag", tag);
  }
</script>

<div
  class="pop"
  bind:this={root}
  role="dialog"
  aria-label={t("tags.style.title", { name: tag })}
  tabindex="-1"
  {onkeydown}
>
  {#if status !== null}
    <p class="status">
      <StatusMark {status} words={statusWords} silent /><span>{statusWords}</span>
    </p>
  {/if}
  {#if promote}
    <div class="official">
      <PromoteAsk kind="tag" name={tag} done={() => void promoted()} />
    </div>
  {:else if status === "official"}
    <div class="official">
      {#if fixed}<p class="note">{t("tags.officialLook", { root: projects.rootName })}</p>{/if}
      <button type="button" class="btn btn--sm" onclick={editList}>{t("tags.officialEdit")}</button>
    </div>
  {/if}
  {#if !fixed}
    <div class="pickers" class:split={status !== null}>
      <LookPicker {color} {icon} pick={(c, i) => void tags.setStyle(tag, c, i)} />
    </div>
  {/if}
</div>

<style>
  .pop {
    position: absolute;
    top: 100%;
    left: 0;
    z-index: 30;
    width: 100%;
    min-width: 220px;
    padding: var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
  }
  .status {
    display: flex;
    align-items: center;
    gap: 4px;
    margin: 0 0 var(--sp-1);
    color: var(--text);
    font-size: var(--fs-ui-sm);
  }
  .note {
    margin: 0 0 var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .official {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--sp-1);
  }
  .split {
    margin-top: var(--sp-2);
    padding-top: var(--sp-1);
    border-top: 1px solid var(--border);
  }
</style>
