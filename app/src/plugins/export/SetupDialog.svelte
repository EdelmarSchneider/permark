<script lang="ts">
  // Page setup, shared by Save PDF… and Print…: paper size, orientation, margins, page numbers. The button of the
  // entry the person came from is the primary one (Enter runs it). The choices are remembered when a button runs.
  import X from "@lucide/svelte/icons/x";
  import type { Margins } from "../../api/generated/Margins";
  import type { Orientation } from "../../api/generated/Orientation";
  import type { PageSize } from "../../api/generated/PageSize";
  import { isComposition } from "../../core/commands/combo";
  import { t, type MessageKey } from "../../core/i18n";
  import { dialogFocus } from "../../core/shell/dialogFocus";
  import Segments from "../../core/shell/settings/Segments.svelte";
  import { exporter, type PaperEntry } from "./store.svelte";

  const SIZES: readonly [PageSize, MessageKey][] = [
    ["a4", "export.size.a4"],
    ["a5", "export.size.a5"],
    ["a3", "export.size.a3"],
    ["letter", "export.size.letter"],
    ["legal", "export.size.legal"],
    ["tabloid", "export.size.tabloid"],
  ];

  const s = $derived(exporter.state.status === "setup" ? exporter.state : null);
  const orientations = $derived<{ value: Orientation; label: string }[]>([
    { value: "portrait", label: t("export.portrait") },
    { value: "landscape", label: t("export.landscape") },
  ]);
  const margins = $derived<{ value: Margins; label: string }[]>([
    { value: "narrow", label: t("export.margins.narrow") },
    { value: "normal", label: t("export.margins.normal") },
    { value: "wide", label: t("export.margins.wide") },
  ]);

  function go(entry: PaperEntry): void {
    void exporter.go(entry);
  }

  /** Enter anywhere but on a footer button runs the primary button (a radio | the checkbox keep their own Space). */
  function onkeydown(event: KeyboardEvent): void {
    if (event.key !== "Enter" || isComposition(event) || !s || s.busy) return;
    if (event.target instanceof Element && event.target.closest("footer")) return;
    event.preventDefault();
    go(s.entry);
  }
</script>

{#if s}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="backdrop"
    onclick={(e) => {
      if (e.target === e.currentTarget) exporter.cancel();
    }}
  >
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="export-setup-title"
      tabindex="-1"
      {onkeydown}
      use:dialogFocus={{ initial: ".btn--primary", onEscape: () => exporter.cancel() }}
    >
      <header>
        <h2 id="export-setup-title">{t("export.setup.title")}</h2>
        <button
          type="button"
          class="icon-btn icon-btn--sm"
          aria-label={t("common.close")}
          title={t("common.close")}
          onclick={() => exporter.cancel()}
        >
          <X size={16} />
        </button>
      </header>

      <div class="rows">
        <label class="name" for="export-size">{t("export.size")}</label>
        <select
          id="export-size"
          class="field-select"
          value={s.setup.size}
          onchange={(e) => {
            const size = SIZES.find(([id]) => id === e.currentTarget.value)?.[0];
            if (size) exporter.change({ size });
          }}
        >
          {#each SIZES as [id, key] (id)}
            <option value={id}>{t(key)}</option>
          {/each}
        </select>

        <span class="name" id="export-orientation">{t("export.orientation")}</span>
        <Segments
          options={orientations}
          value={s.setup.orientation}
          labelledby="export-orientation"
          onchange={(orientation) => exporter.change({ orientation })}
        />

        <span class="name" id="export-margins">{t("export.margins")}</span>
        <Segments
          options={margins}
          value={s.setup.margins}
          labelledby="export-margins"
          onchange={(value) => exporter.change({ margins: value })}
        />

        <span></span>
        <div class="check">
          <label>
            <input
              type="checkbox"
              checked={s.capabilities.pageNumbers && s.setup.pageNumbers}
              disabled={!s.capabilities.pageNumbers}
              aria-describedby={s.capabilities.pageNumbers ? undefined : "export-numbers-note"}
              onchange={(e) => exporter.change({ pageNumbers: e.currentTarget.checked })}
            />
            {t("export.pageNumbers")}
          </label>
          {#if !s.capabilities.pageNumbers}
            <span class="note" id="export-numbers-note">{t("export.pageNumbers.unavailable")}</span>
          {/if}
        </div>
      </div>

      <p class="note">{t("export.lightNote")}</p>
      {#if !s.capabilities.pdf && !s.capabilities.print}
        <p class="note" role="alert">{t("export.unsupported")}</p>
      {/if}

      <footer>
        <button type="button" class="btn btn--sm" onclick={() => exporter.cancel()}>
          {t("common.cancel")}
        </button>
        <button
          type="button"
          class="btn btn--sm"
          class:btn--primary={s.entry === "pdf"}
          disabled={s.busy || !s.capabilities.pdf}
          onclick={() => go("pdf")}
        >
          {t("export.savePdf")}
        </button>
        <button
          type="button"
          class="btn btn--sm"
          class:btn--primary={s.entry === "print"}
          disabled={s.busy || !s.capabilities.print}
          onclick={() => go("print")}
        >
          {t("export.print")}
        </button>
      </footer>
    </div>
  </div>
{/if}

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 50;
    display: grid;
    place-items: center;
    padding: var(--sp-5);
    background: var(--backdrop);
    animation: pm-fade var(--dur) var(--ease);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    width: min(440px, 100%);
    padding: var(--sp-4) var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    box-shadow: var(--shadow-pop);
    outline: none;
    font-size: var(--fs-ui);
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
  }
  h2 {
    margin: 0;
    font-size: 15px;
    font-weight: 600;
  }
  .rows {
    display: grid;
    grid-template-columns: max-content minmax(0, 1fr);
    align-items: center;
    gap: var(--sp-3) var(--sp-4);
  }
  .name {
    color: var(--muted);
  }
  .rows :global(.segments) {
    justify-self: start;
  }
  .rows select {
    justify-self: start;
    max-width: 100%;
  }
  .check {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .check label {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .note {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
</style>
