<script lang="ts">
  // "Show earlier versions": a small dialog that lists the versions of a note, newest first, shows the one selected and
  // restores it. The text the note has now is kept as a version first, so a restore can be undone.
  import { onMount } from "svelte";
  import { formatNumber, intlTag, t } from "../i18n";
  import { registerOverlay, rememberFocus } from "../overlays";
  import { notes } from "../stores/notes.svelte";
  import { clock as formatClock } from "../time";

  let dialog: HTMLElement | undefined = $state();
  let restore: HTMLButtonElement | undefined = $state();

  const view = $derived(notes.versions);
  const name = $derived(
    notes.entries.find((n) => n.path === view?.path)?.title || (view?.path ?? ""),
  );

  /** "Oct 15, 14:32" in the language of the interface; the year too when it is not this one. */
  function when(ms: number): string {
    const d = new Date(ms);
    const sameYear = d.getFullYear() === new Date().getFullYear();
    const day = d.toLocaleDateString(intlTag(), {
      month: "short",
      day: "numeric",
      ...(sameYear ? {} : { year: "numeric" }),
    });
    return `${day}, ${formatClock(ms)}`;
  }

  function size(bytes: number): string {
    return bytes < 1024
      ? `${formatNumber(bytes)} B`
      : `${formatNumber(Math.round(bytes / 1024))} KB`;
  }

  function close(): void {
    notes.closeVersions();
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Tab" && dialog) {
      const focusable = Array.from(
        dialog.querySelectorAll<HTMLElement>("button:not(:disabled), [tabindex='0']"),
      );
      const first = focusable[0];
      const last = focusable[focusable.length - 1];
      if (!first || !last) return;
      if (e.shiftKey && document.activeElement === first) {
        e.preventDefault();
        last.focus();
      } else if (!e.shiftKey && document.activeElement === last) {
        e.preventDefault();
        first.focus();
      }
      return;
    }
    if ((e.key === "ArrowDown" || e.key === "ArrowUp") && view && view.items.length > 0) {
      e.preventDefault();
      const at = view.items.findIndex((v) => v.id === view.selected);
      const next =
        view.items[
          Math.min(view.items.length - 1, Math.max(0, at + (e.key === "ArrowDown" ? 1 : -1)))
        ];
      if (next) void notes.selectVersion(next.id);
    }
  }

  onMount(() => {
    const back = rememberFocus();
    dialog?.focus();
    const unregister = registerOverlay(close);
    return () => {
      unregister();
      back();
    };
  });
</script>

{#if view}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="backdrop"
    onclick={(e) => {
      if (e.target === e.currentTarget) close();
    }}
  >
    <div
      bind:this={dialog}
      class="dialog notes-versions"
      role="dialog"
      aria-modal="true"
      aria-labelledby="versions-title"
      tabindex="-1"
      {onkeydown}
    >
      <h2 id="versions-title">{t("notes.versions.of", { name })}</h2>
      {#if view.failed}
        <p class="inline-error" role="alert">{view.failed}</p>
      {/if}
      {#if view.items.length === 0 && !view.loading}
        <p class="notes-versions__empty">{t("notes.versions.empty")}</p>
      {:else}
        <div class="notes-versions__body">
          <ul class="notes-versions__list" aria-label={t("notes.versions.list")}>
            {#each view.items as item (item.id)}
              <li>
                <button
                  type="button"
                  class={["notes-versions__item", item.id === view.selected && "is-selected"]}
                  aria-current={item.id === view.selected ? "true" : undefined}
                  onclick={() => void notes.selectVersion(item.id)}
                >
                  <span>{when(item.writtenMs)}</span>
                  <span class="notes-versions__size">{size(item.sizeBytes)}</span>
                </button>
              </li>
            {/each}
          </ul>
          <pre
            class="notes-versions__preview"
            aria-label={t("notes.versions.preview")}
            aria-busy={view.loading}>{view.loading
              ? t("notes.versions.loading")
              : (view.text ?? "")}</pre>
        </div>
      {/if}
      <p class="notes-versions__hint">{t("notes.versions.restoreHint")}</p>
      <footer>
        <button type="button" class="btn" onclick={close}>{t("common.close")}</button>
        <button
          bind:this={restore}
          type="button"
          class="btn btn--primary"
          disabled={!view.selected || view.loading || notes.readOnly !== null}
          onclick={() => void notes.restoreVersion()}
        >
          {t("notes.versions.restore")}
        </button>
      </footer>
    </div>
  </div>
{/if}
