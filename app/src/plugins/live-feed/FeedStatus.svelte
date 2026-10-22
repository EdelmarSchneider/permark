<script lang="ts">
  // The feed's item in the status bar (opens the Changes panel; counts the outside changes not seen yet) and the
  // toast that says documents changed while the person reads something else. Quiet: no sound, goes away by itself
  // (not while the pointer is over it or the focus inside). Nothing at all until the project calls for the feed.
  import { documents } from "../../core/doc/document.svelte";
  import { t, tn } from "../../core/i18n";
  import { rightViews } from "../../core/shell/rightViews.svelte";
  import { followStore } from "./followLink.svelte";
  import { ACTIVE_MS, countText, feed, PANEL } from "./store.svelte";
  import { split } from "./time";

  let now = $state(Date.now());
  $effect(() => {
    const timer = setInterval(() => (now = Date.now()), 5000);
    return () => clearInterval(timer);
  });
  const active = $derived(feed.lastAt > 0 && now - feed.lastAt < ACTIVE_MS);
  const follow = $derived(followStore());

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

  function followLatest(path: string): void {
    follow?.followTo(path);
    feed.dismissToast();
  }

  /** The focus left the toast (not to another of its buttons): it gets its time again. */
  function focusOut(event: FocusEvent): void {
    const to = event.relatedTarget;
    if (
      to instanceof Node &&
      event.currentTarget instanceof Node &&
      event.currentTarget.contains(to)
    ) {
      return;
    }
    feed.releaseToast();
  }
</script>

{#if feed.called}
  <button
    type="button"
    class="chip"
    aria-pressed={rightViews.onScreen(PANEL)}
    aria-label={feed.unseen > 0
      ? tn("live.chipLabel", feed.unseen, { n: countText(feed.unseen) })
      : t("live.label")}
    title={t("live.label")}
    onclick={() => feed.toggle()}
  >
    <span class="dot" class:active aria-hidden="true"></span>
    {feed.unseen > 0 ? t("live.chipCount", { n: countText(feed.unseen) }) : t("live.chip")}
  </button>
{/if}

{#if feed.toast}
  {@const toast = feed.toast}
  <div
    class="toast"
    role="status"
    aria-live="polite"
    onpointerenter={() => feed.holdToast()}
    onpointerleave={() => feed.releaseToast()}
    onfocusin={() => feed.holdToast()}
    onfocusout={focusOut}
  >
    <button type="button" class="open" onclick={() => openLatest(toast.path)}>
      <strong>{tn("toast.docsChanged", toast.count)}</strong>
      <span class="latest"
        >{t("toast.latest")}: {split(toast.path).name} {t(KINDS[toast.kind])}</span
      >
    </button>
    {#if follow !== null && !follow.on}
      <button type="button" class="follow" onclick={() => followLatest(toast.path)}
        >{t("toast.follow")}</button
      >
    {/if}
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
  /* idle: full --muted (non-text contrast >= 3:1 on the status bar); active: the accent. No animation. */
  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--muted);
  }
  .dot.active {
    background: var(--accent);
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
  .follow {
    padding: 0 var(--sp-3);
    border: 0;
    border-left: 1px solid var(--border);
    background: transparent;
    color: var(--accent-text);
    font: inherit;
    font-weight: 600;
    cursor: pointer;
  }
  .follow:hover {
    background: var(--hover);
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
