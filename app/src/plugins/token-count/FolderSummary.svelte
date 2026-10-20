<script lang="ts">
  // The result of a folder count: the total with its tokenizer (exact or approximate), the files counted and the ones
  // left out (and why), the file types, the biggest files, how long it took. Copy summary puts it on the clipboard as
  // plain text; Count again reads only what changed.
  import X from "@lucide/svelte/icons/x";
  import { language, t, tn } from "../../core/i18n";
  import { skipKey, skippedReasons, typeName, type Say } from "./folders";
  import { folders } from "./folders.svelte";
  import { infoOf } from "./tokenizers";

  const summary = $derived(folders.summary);
  const info = $derived(summary === null ? null : infoOf(summary.tokenizer));
  const stale = $derived(summary !== null && folders.isStale(summary.folder));

  const formatter = $derived(new Intl.NumberFormat(language.locale));
  const number = (n: number): string => formatter.format(n);
  const say: Say = {
    t,
    tn,
    number: (n) => new Intl.NumberFormat(language.locale).format(n),
  };

  function seconds(millis: number): string {
    return new Intl.NumberFormat(language.locale, { maximumFractionDigits: 1 }).format(
      Math.max(0.1, millis / 1000),
    );
  }

  let dialog: HTMLElement | undefined = $state();

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      folders.closeSummary();
    }
  }

  // focus enters the dialog and returns where it was when the dialog goes
  $effect(() => {
    if (summary === null) return;
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    dialog?.focus();
    return () => before?.focus();
  });
</script>

{#if summary !== null}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="backdrop"
    onclick={(e) => {
      if (e.target === e.currentTarget) folders.closeSummary();
    }}
  >
    <div
      bind:this={dialog}
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="folder-summary-title"
      tabindex="-1"
      {onkeydown}
    >
      <header>
        <h2 id="folder-summary-title">
          {t("tokens.folder.summary.title", { folder: folders.nameOf(summary.folder) })}
        </h2>
        <button
          type="button"
          class="icon-btn icon-btn--sm"
          aria-label={t("common.close")}
          title={t("common.close")}
          onclick={() => folders.closeSummary()}
        >
          <X size={16} />
        </button>
      </header>

      <p class="total">
        <span class="figure">
          {summary.approx ? "≈ " : ""}{tn("tokens.count", summary.tokens, {
            n: number(summary.tokens),
          })}
        </span>
        <span class="by">· {info?.short ?? summary.tokenizer}</span>
        <span class="badge" class:is-exact={!summary.approx}>
          {summary.approx ? t("tokens.badge.approx") : t("tokens.badge.exact")}
        </span>
      </p>
      {#if summary.approx}
        <p class="why">
          {info !== null && !info.exact ? t(info.why) : t("tokens.folder.summary.interim")}
        </p>
      {/if}
      {#if stale}
        <p class="stale">{t("tokens.folder.summary.stale")}</p>
      {/if}

      <p class="files">
        {tn("tokens.folder.summary.counted", summary.counted, {
          n: number(summary.counted),
        })}{summary.skipped > 0
          ? `, ${tn("tokens.folder.summary.skipped", summary.skipped, { n: number(summary.skipped) })}`
          : ""}
      </p>
      {#if summary.skipped > 0}
        <p class="reasons">{skippedReasons(summary, say)}</p>
        <details class="skipped">
          <summary>{t("tokens.folder.summary.skippedList")}</summary>
          <ul>
            {#each summary.skippedList as file (file.path)}
              <li>
                <span class="path">{file.path}</span>
                <span class="reason">{t(skipKey(file.reason))}</span>
              </li>
            {/each}
            {#if summary.skipped > summary.skippedList.length}
              <li class="more">
                {t("tokens.folder.summary.more", {
                  n: number(summary.skipped - summary.skippedList.length),
                })}
              </li>
            {/if}
          </ul>
        </details>
      {/if}

      {#if summary.byType.length > 0}
        <h3>{t("tokens.folder.summary.types")}</h3>
        <table>
          <tbody>
            {#each summary.byType as row (row.ext)}
              <tr>
                <th scope="row">{typeName(row.ext, say)}</th>
                <td class="num">{number(row.tokens)}</td>
                <td class="faint">
                  {tn("tokens.folder.files", row.files, { n: number(row.files) })}
                </td>
              </tr>
            {/each}
          </tbody>
        </table>
      {/if}

      {#if summary.top.length > 0}
        <h3>{t("tokens.folder.summary.top")}</h3>
        <ol class="top">
          {#each summary.top as file (file.path)}
            <li>
              <span class="path" title={file.path}>{file.path}</span>
              <span class="num">{number(file.tokens)}</span>
            </li>
          {/each}
        </ol>
      {/if}

      <p class="took">
        {t("tokens.folder.summary.took", { seconds: seconds(summary.millis) })}{summary.cached > 0
          ? ` · ${tn("tokens.folder.summary.cached", summary.cached, { n: number(summary.cached) })}`
          : ""}
      </p>

      <footer>
        <button type="button" class="btn" onclick={() => void folders.copy()}>
          {t("tokens.folder.summary.copy")}
        </button>
        <button type="button" class="btn" onclick={() => void folders.again()}>
          {t("tokens.folder.summary.again")}
        </button>
        <button type="button" class="btn btn--primary" onclick={() => folders.closeSummary()}>
          {t("common.close")}
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
    width: min(560px, 100%);
    max-height: 100%;
    overflow-y: auto;
    padding: var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-pop);
    font-size: var(--fs-ui-sm);
    outline: none;
  }
  header {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
    margin-bottom: var(--sp-3);
  }
  h2 {
    margin: 0;
    overflow: hidden;
    font-size: 15px;
    font-weight: 600;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  h3 {
    margin: var(--sp-4) 0 var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  p {
    margin: 0 0 var(--sp-2);
  }
  .total {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: var(--sp-2);
  }
  .figure {
    font-size: 22px;
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  .by {
    color: var(--muted);
  }
  .badge {
    padding: 0 6px;
    border: 1px solid var(--git-modified-text);
    border-radius: 999px;
    color: var(--git-modified-text);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    line-height: 1.5;
  }
  .badge.is-exact {
    border-color: var(--git-added-text);
    color: var(--git-added-text);
  }
  .why,
  .reasons,
  .took {
    color: var(--muted);
  }
  .stale {
    color: var(--git-modified-text);
  }
  .skipped {
    margin-bottom: var(--sp-2);
  }
  .skipped summary {
    cursor: pointer;
    color: var(--muted);
  }
  ul,
  ol {
    margin: var(--sp-1) 0 0;
    padding: 0;
    list-style: none;
  }
  .skipped ul {
    max-height: 160px;
    overflow-y: auto;
  }
  li {
    display: flex;
    justify-content: space-between;
    gap: var(--sp-3);
    padding: 1px 0;
  }
  .path {
    min-width: 0;
    overflow: hidden;
    font-family: var(--font-code);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .reason,
  .more,
  .faint {
    color: var(--muted);
    white-space: nowrap;
  }
  table {
    width: 100%;
    border-collapse: collapse;
  }
  th {
    padding: 1px var(--sp-3) 1px 0;
    font-family: var(--font-code);
    font-weight: 400;
    text-align: left;
  }
  td {
    padding: 1px 0 1px var(--sp-3);
  }
  .num {
    font-variant-numeric: tabular-nums;
    text-align: right;
  }
  .took {
    margin-top: var(--sp-4);
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
    margin-top: var(--sp-4);
  }
</style>
