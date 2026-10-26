<script lang="ts">
  // The converter: the source as read (the person can say it is another format), every target with the tokens it
  // would take and the change from the source, the best one marked, an honest word on each, a preview, what is lost.
  // Copy | Save as a new file next to the source | Open the result. The source is never written.
  import X from "@lucide/svelte/icons/x";
  import { language, t, tn } from "../../core/i18n";
  import {
    change,
    count,
    noteKey,
    reasonKey,
    sourceKey,
    SOURCES,
    targetKey,
    tokenizerName,
  } from "./model";
  import { converter } from "./store.svelte";

  const s = $derived(converter.state);
  const ready = $derived(s.status === "ready" ? s : null);
  const selected = $derived(ready?.analysis.targets.find((t) => t.id === ready.selected) ?? null);
  const name = $derived(
    s.status === "closed"
      ? ""
      : s.origin.kind === "file"
        ? (s.origin.path.split("/").at(-1) ?? s.origin.path)
        : t("convert.fromPage"),
  );
  const tokenizer = $derived(tokenizerName(ready?.analysis.tokenizer ?? null));

  let dialog: HTMLElement | undefined = $state();

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      converter.close();
    }
  }

  // focus enters the dialog and returns where it was when the dialog goes
  $effect(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog?.focus();
    return () => before?.focus();
  });

  function size(bytes: number): string {
    const format = new Intl.NumberFormat(language.locale, { maximumFractionDigits: 1 });
    if (bytes < 1024) return t("convert.bytes", { n: format.format(bytes) });
    if (bytes < 1024 * 1024) return t("convert.kb", { n: format.format(bytes / 1024) });
    return t("convert.mb", { n: format.format(bytes / 1024 / 1024) });
  }
</script>

{#if s.status !== "closed"}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="backdrop"
    onclick={(e) => {
      if (e.target === e.currentTarget) converter.close();
    }}
  >
    <div
      bind:this={dialog}
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="convert-title"
      tabindex="-1"
      {onkeydown}
    >
      <header>
        <h2 id="convert-title">{t("convert.title", { name })}</h2>
        <button
          type="button"
          class="icon-btn icon-btn--sm"
          aria-label={t("convert.close")}
          title={t("convert.close")}
          onclick={() => converter.close()}
        >
          <X size={16} />
        </button>
      </header>

      <div class="source">
        <label>
          {t("convert.readAs")}
          <select
            value={s.from ?? ""}
            onchange={(e) => converter.readAs(e.currentTarget.value)}
            disabled={s.status === "working"}
          >
            {#if s.from === null}<option value="">{t("convert.detect")}</option>{/if}
            {#each SOURCES as id (id)}
              <option value={id}>{t(sourceKey(id))}</option>
            {/each}
          </select>
        </label>
        {#if ready}
          <span class="facts">
            {size(ready.analysis.bytes)} ·
            {tn("convert.tokens", ready.analysis.sourceTokens, {
              count: count(ready.analysis.sourceTokens, ready.analysis.approx, language.locale),
            })}
            {#if tokenizer}<span class="tokenizer">{tokenizer}</span>{/if}
          </span>
        {/if}
      </div>

      {#if s.status === "working"}
        <div class="state" role="status">
          <span>{t("convert.working")}</span>
          <button type="button" class="btn btn--sm" onclick={() => converter.cancel()}>
            {t("convert.cancel")}
          </button>
        </div>
      {:else if s.status === "cancelled"}
        <p class="state" role="status">{t("convert.cancelled")}</p>
      {:else if s.status === "refused"}
        <p class="state refused" role="alert">
          {s.reason === "tooLarge"
            ? t("convert.tooLarge")
            : s.reason === "invalid"
              ? t("convert.invalid", { format: t(sourceKey(s.from ?? "")) })
              : t("convert.failed")}
          {#if s.line > 0}
            · {t("note.problemAt", { line: s.line })}{/if}
          {#if s.detail}<code>{s.detail}</code>{/if}
        </p>
      {:else if ready}
        <div class="split">
          <div class="targets" role="radiogroup" aria-label={t("convert.targets")}>
            {#each ready.analysis.targets as target (target.id)}
              <button
                type="button"
                role="radio"
                aria-checked={ready.selected === target.id}
                class="target"
                class:is-selected={ready.selected === target.id}
                class:is-out={target.fit === "no"}
                disabled={target.fit === "no"}
                onclick={() => converter.select(target.id)}
              >
                <span class="line">
                  <span class="tname">{t(targetKey(target.id))}</span>
                  {#if target.best}<span class="best">{t("convert.best")}</span>{/if}
                  {#if target.tokens !== null}
                    <span class="num">
                      {count(target.tokens, ready.analysis.approx, language.locale)}
                    </span>
                    <span
                      class="change"
                      class:is-gain={target.tokens < ready.analysis.sourceTokens}
                      class:is-loss={target.tokens >= ready.analysis.sourceTokens}
                    >
                      {change(target.tokens, ready.analysis.sourceTokens)}
                    </span>
                  {/if}
                </span>
                <span class="why" class:is-poor={target.fit !== "good"}>
                  {target.fit === "good"
                    ? t("convert.fits")
                    : target.fit === "poor"
                      ? t("convert.poorFit")
                      : t("convert.notAvailable")}: {t(reasonKey(target.reason))}
                </span>
              </button>
            {/each}
          </div>
          <div class="preview">
            <p class="preview-title">{t("convert.preview")}</p>
            <pre>{selected?.preview ?? ""}</pre>
          </div>
        </div>
        {@const notes = [...ready.analysis.notes, ...(selected?.notes ?? [])]}
        {#if notes.length > 0}
          <p class="notes">
            {t("convert.lost")}
            {[...new Set(notes)].map((n) => t(noteKey(n))).join(" · ")}
          </p>
        {/if}
        <p class="estimate">
          {ready.analysis.approx ? t("convert.approxHint") : t("convert.exactHint")}
        </p>
      {/if}

      <footer>
        {#if ready}
          {#if ready.action.kind === "done"}
            <span class="result" role="status">
              {ready.action.path === null
                ? t("convert.copied")
                : t("convert.saved", { path: ready.action.path })}
            </span>
          {:else if ready.action.kind === "failed"}
            <span class="result failed" role="alert">{t("convert.actionFailed")}</span>
          {/if}
          <button
            type="button"
            class="btn btn--sm"
            disabled={selected === null || selected.fit === "no" || ready.action.kind === "busy"}
            onclick={() => void converter.act("copy")}
          >
            {t("convert.copy")}
          </button>
          <button
            type="button"
            class="btn btn--sm"
            disabled={selected === null || selected.fit === "no" || ready.action.kind === "busy"}
            onclick={() => void converter.act("save")}
          >
            {t("convert.save")}
          </button>
          <button
            type="button"
            class="btn btn--sm btn--primary"
            disabled={selected === null || selected.fit === "no" || ready.action.kind === "busy"}
            onclick={() => void converter.act("open")}
          >
            {t("convert.open")}
          </button>
        {:else}
          <button type="button" class="btn btn--sm" onclick={() => converter.close()}>
            {t("convert.close")}
          </button>
        {/if}
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
    width: min(880px, 100%);
    max-height: min(640px, 100%);
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
    overflow-wrap: anywhere;
  }
  .source {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2) var(--sp-4);
    color: var(--muted);
  }
  .source label {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .facts {
    font-variant-numeric: tabular-nums;
  }
  .tokenizer {
    margin-left: var(--sp-1);
    padding: 0 6px;
    border-radius: 8px;
    background: var(--surface-2);
    font-size: var(--fs-ui-xs);
  }
  .state {
    display: flex;
    align-items: center;
    gap: var(--sp-3);
    margin: 0;
    padding: var(--sp-5) 0;
    color: var(--muted);
  }
  .state.refused {
    display: block;
    color: var(--git-deleted-text);
  }
  .state code {
    font-family: var(--font-code);
    overflow-wrap: anywhere;
  }
  .split {
    display: grid;
    grid-template-columns: minmax(250px, 340px) minmax(0, 1fr);
    gap: var(--sp-3);
    min-height: 0;
    flex: 1;
  }
  .targets {
    display: flex;
    flex-direction: column;
    gap: 2px;
    overflow-y: auto;
  }
  .target {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 10px;
    border: 1px solid transparent;
    border-radius: var(--radius-control);
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .target:hover:not(:disabled) {
    background: var(--hover);
  }
  .target.is-selected {
    border-color: var(--accent-muted);
    background: var(--accent-soft);
  }
  .target:focus-visible {
    outline: 2px solid var(--accent);
  }
  .target.is-out {
    cursor: default;
    opacity: 0.7;
  }
  .line {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
  }
  .tname {
    font-weight: 600;
  }
  .best {
    padding: 0 6px;
    border-radius: 8px;
    background: var(--accent);
    color: var(--accent-fg);
    font-size: var(--fs-ui-xs);
  }
  .num {
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }
  .change {
    min-width: 4.2em;
    text-align: right;
    font-variant-numeric: tabular-nums;
  }
  .change.is-gain {
    color: var(--git-added-text);
  }
  .change.is-loss {
    color: var(--git-deleted-text);
  }
  .why {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .why.is-poor {
    color: var(--git-modified-text);
  }
  .target.is-out .why {
    color: var(--faint);
  }
  .preview {
    display: flex;
    flex-direction: column;
    min-width: 0;
    min-height: 0;
  }
  .preview-title {
    margin: 0 0 var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  pre {
    flex: 1;
    min-height: 12em;
    margin: 0;
    padding: var(--sp-2) var(--sp-3);
    overflow: auto;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
    font: 12.5px / 1.5 var(--font-code);
    white-space: pre;
  }
  .notes,
  .estimate {
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
  .result {
    margin-right: auto;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
  }
  .result.failed {
    color: var(--git-deleted-text);
  }
  @media (max-width: 640px) {
    .split {
      grid-template-columns: minmax(0, 1fr);
    }
  }
</style>
