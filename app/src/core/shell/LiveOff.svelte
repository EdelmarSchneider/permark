<script lang="ts">
  // Live updates off in the status bar (`liveSignal.svelte.ts`): said once to screen readers when it comes (here, also
  // while the Live chip shows it), and drawn here only when no chip claims it. A short label that fits a narrow bar
  // in every language; the why + what to do open below it on a click | Enter (keyboard too), Esc closes.
  import TriangleAlert from "@lucide/svelte/icons/triangle-alert";
  import { t } from "../i18n";
  import { liveOff } from "./liveSignal.svelte";

  let open = $state(false);
  let button: HTMLButtonElement | undefined = $state();
  let at = $state({ left: 0, bottom: 0 });

  const shown = $derived(liveOff.off && !liveOff.claimed);
  $effect(() => {
    if (!shown) open = false;
  });

  function toggle(): void {
    if (!open && button) {
      const box = button.getBoundingClientRect();
      at = { left: box.left, bottom: window.innerHeight - box.top + 6 };
    }
    open = !open;
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.key === "Escape" && open) {
      event.preventDefault();
      event.stopPropagation();
      open = false;
    }
  }
</script>

<span class="sr-only" role="status"
  >{#if liveOff.off}{t("status.liveOff")}. {t("status.liveOffHint")}{/if}</span
>
{#if shown}
  <span class="live-off">
    <button
      bind:this={button}
      type="button"
      class="item"
      aria-label={t("status.liveOff")}
      aria-expanded={open}
      aria-controls="live-off-hint"
      title={t("status.liveOffHint")}
      onclick={toggle}
      {onkeydown}
      onblur={() => (open = false)}
    >
      <TriangleAlert size={13} aria-hidden="true" />
      {t("status.liveOffShort")}
    </button>
    {#if open}
      <p class="hint" id="live-off-hint" style:left="{at.left}px" style:bottom="{at.bottom}px">
        {t("status.liveOffHint")}
      </p>
    {/if}
  </span>
{/if}

<style>
  .live-off {
    display: contents;
  }
  .item {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: var(--git-modified-text);
    font: inherit;
    white-space: nowrap;
    cursor: pointer;
  }
  .item:hover,
  .item[aria-expanded="true"] {
    background: var(--hover);
  }
  .hint {
    position: fixed;
    z-index: 50;
    max-width: min(340px, calc(100vw - 2 * var(--sp-4)));
    margin: 0;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-pop);
    font-size: var(--fs-ui-sm);
    white-space: normal;
  }
</style>
