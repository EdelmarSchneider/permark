<script lang="ts">
  // The update chip of the status bar, only while something waits for the person: a new version, the download, a
  // restart to finish, a failure. A click opens the version, its notes and the actions (or Later).
  import Download from "@lucide/svelte/icons/download";
  import { t } from "../../core/i18n";
  import { actionOf, chipOf, statusLine, type Action } from "./model";
  import { updates } from "./store.svelte";

  const current = $derived(updates.state);
  const chip = $derived(chipOf(current));
  const action = $derived(actionOf(current));
  const notes = $derived(current.state === "available" ? current.notes : null);

  const LABELS: Readonly<Record<Action, "updates.check" | "updates.download" | "updates.restart">> =
    {
      check: "updates.check",
      download: "updates.download",
      restart: "updates.restart",
    };

  let box: HTMLElement | undefined = $state();

  /** The status bar clips what overflows it: the popover is fixed, above the chip, inside the window. */
  function above(node: HTMLElement): void {
    if (!box) return;
    const anchor = box.getBoundingClientRect();
    const left = Math.min(anchor.left, window.innerWidth - node.offsetWidth - 8);
    node.style.left = `${String(Math.max(8, left))}px`;
    node.style.bottom = `${String(window.innerHeight - anchor.top + 6)}px`;
  }

  // closes on Escape | a click outside
  $effect(() => {
    if (!updates.popover) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") updates.later();
    };
    const onPointer = (e: PointerEvent): void => {
      if (box && e.target instanceof Node && !box.contains(e.target)) updates.later();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer);
    };
  });
</script>

{#if chip !== null}
  <span class="update" bind:this={box}>
    <button
      type="button"
      class="chip is-{chip.tone}"
      aria-haspopup="dialog"
      aria-expanded={updates.popover}
      title={statusLine(current)}
      onclick={() => (updates.popover = !updates.popover)}
    >
      <Download size={13} aria-hidden="true" />
      <span class="label">{chip.text}</span>
    </button>
    {#if updates.popover}
      <div class="popover" role="dialog" aria-label={t("updates.section")} use:above>
        <p class="title">{t("updates.section")}</p>
        <p class="line" role="status">{statusLine(current)}</p>
        {#if notes !== null}
          <p class="label-row">{t("updates.notes")}</p>
          <p class="notes">{notes}</p>
        {/if}
        <div class="actions">
          <button type="button" class="btn btn--sm" onclick={() => updates.later()}>
            {t("updates.later")}
          </button>
          {#if action !== null}
            <button
              type="button"
              class="btn btn--sm btn--primary"
              onclick={() => void updates.run(action)}
            >
              {t(LABELS[action])}
            </button>
          {/if}
        </div>
      </div>
    {/if}
  </span>
{/if}

<style>
  .update {
    position: relative;
    display: inline-flex;
    align-items: center;
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
  }
  .chip.is-busy {
    color: inherit;
  }
  .chip.is-ready {
    color: var(--git-added-text);
  }
  .chip.is-problem {
    color: var(--git-deleted-text);
  }
  .chip:hover,
  .chip[aria-expanded="true"] {
    background: var(--hover);
  }
  .label {
    max-width: 20ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-variant-numeric: tabular-nums;
  }
  .popover {
    position: fixed;
    z-index: 40;
    width: 320px;
    max-width: calc(100vw - 24px);
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-pop);
    font-size: var(--fs-ui-sm);
    white-space: normal;
  }
  .title {
    margin: 0 0 var(--sp-2);
    font-weight: 600;
  }
  .line {
    margin: 0;
  }
  .label-row {
    margin: var(--sp-3) 0 4px;
    color: var(--muted);
  }
  .notes {
    max-height: 160px;
    margin: 0;
    overflow-y: auto;
    color: var(--muted);
    white-space: pre-line;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    gap: var(--sp-2);
    margin-top: var(--sp-3);
  }
</style>
