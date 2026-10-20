<script lang="ts">
  // The feed's item in the status bar (opens the Changes panel) and the toast that says documents changed while the
  // person reads something else. Quiet: no sound, goes away by itself.
  import { documents } from "../../core/doc/document.svelte";
  import { t, tn } from "../../core/i18n";
  import { ui } from "../../core/shell/ui.svelte";
  import { ACTIVE_MS, feed } from "./store.svelte";
  import { split } from "./time";

  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(timer);
  });
  const active = $derived(feed.lastAt > 0 && now - feed.lastAt < ACTIVE_MS);

  const KINDS = {
    created: "toast.kind.created",
    modified: "toast.kind.modified",
    removed: "toast.kind.removed",
    renamed: "toast.kind.renamed",
  } as const;

  function openLatest(path: string): void {
    documents.navigate(path, null);
    feed.dismissToast();
  }
</script>

<button
  type="button"
  class="chip"
  aria-pressed={ui.right === "live-feed.panel"}
  title={t("live.label")}
  onclick={() => ui.toggleRight("live-feed.panel")}
>
  <span class="dot" class:active aria-hidden="true"></span>
  {t("live.chip")}
</button>

{#if feed.toast}
  {@const toast = feed.toast}
  <div class="toast" role="status" aria-live="polite">
    <button type="button" class="open" onclick={() => openLatest(toast.path)}>
      <strong>{tn("toast.docsChanged", toast.count)}</strong>
      <span class="latest"
        >{t("toast.latest")}: {split(toast.path).name} {t(KINDS[toast.kind])}</span
      >
    </button>
    <button
      type="button"
      class="close"
      aria-label={t("toast.dismiss")}
      onclick={() => feed.dismissToast()}>×</button
    >
  </div>
{/if}

<style>
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .chip:hover,
  .chip[aria-pressed="true"] {
    background: var(--hover);
    color: var(--text);
  }
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
    opacity: 0.5;
  }
  .dot.active {
    background: var(--accent);
    opacity: 1;
  }
  .toast {
    position: fixed;
    right: var(--sp-4);
    bottom: calc(var(--h-status) + var(--sp-3));
    z-index: 50;
    display: flex;
    align-items: stretch;
    max-width: min(420px, calc(100vw - 2 * var(--sp-4)));
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-pop);
    font-size: var(--fs-ui-sm);
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  .open {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 2px;
    padding: var(--sp-2) var(--sp-3);
    border: 0;
    background: transparent;
    color: inherit;
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .open:hover {
    background: var(--hover);
  }
  .latest {
    max-width: 36ch;
    overflow: hidden;
    color: var(--muted);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .close {
    padding: 0 var(--sp-3);
    border: 0;
    border-left: 1px solid var(--border);
    background: transparent;
    color: var(--muted);
    font-size: 16px;
    cursor: pointer;
  }
  .close:hover {
    color: var(--text);
  }
</style>
