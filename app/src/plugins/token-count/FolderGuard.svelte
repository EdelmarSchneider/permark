<script lang="ts">
  // The guard of a folder count: a big folder asks first (how many files, how many bytes, about how long), a folder that
  // is too big says so and why. Enter = the answer in bold, Escape | Cancel | backdrop = no.
  import { language, t, tn } from "../../core/i18n";
  import { dialogFocus } from "../../core/shell/dialogFocus";
  import { REFUSE_FILES, timeUnit } from "./folders";
  import { folders } from "./folders.svelte";
  import { formatMegabytes } from "./model";

  const job = $derived(folders.job);
  const ask = $derived(job.state === "confirm" ? job : null);
  const refused = $derived(job.state === "refused" ? job : null);
  const open = $derived(ask !== null || refused !== null);

  const number = (n: number): string => new Intl.NumberFormat(language.locale).format(n);

  const time = $derived.by((): string => {
    if (ask === null) return "";
    const { unit, n } = timeUnit(ask.seconds);
    return unit === "seconds"
      ? tn("tokens.folder.seconds", n, { n: number(n) })
      : tn("tokens.folder.minutes", n, { n: number(n) });
  });

  function no(): void {
    if (ask !== null) void folders.cancel();
    else void folders.dismiss();
  }
</script>

{#if open}
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <div
    class="backdrop"
    onclick={(e) => {
      if (e.target === e.currentTarget) no();
    }}
  >
    <div
      class="dialog"
      role="dialog"
      aria-modal="true"
      aria-labelledby="folder-guard-title"
      aria-describedby="folder-guard-body"
      tabindex="-1"
      use:dialogFocus={{ initial: "[data-initial]", onEscape: no }}
    >
      {#if ask !== null}
        <h2 id="folder-guard-title">
          {t("tokens.folder.guard.title", { folder: folders.nameOf(ask.folder) })}
        </h2>
        <div id="folder-guard-body">
          <p>
            {t("tokens.folder.guard.body", {
              files: tn("tokens.folder.files", ask.files, { n: number(ask.files) }),
              size: formatMegabytes(ask.bytes, language.locale),
              time,
            })}
          </p>
          <p class="note">{t("tokens.folder.guard.safe")}</p>
        </div>
        <footer>
          <button type="button" class="btn" onclick={no}>{t("common.cancel")}</button>
          <button
            data-initial
            type="button"
            class="btn btn--primary"
            onclick={() => void folders.confirm()}
          >
            {t("tokens.folder.guard.go")}
          </button>
        </footer>
      {:else if refused !== null}
        <h2 id="folder-guard-title">{t("tokens.folder.refused.title")}</h2>
        <div id="folder-guard-body">
          <p>
            {refused.reason === "files"
              ? t("tokens.folder.refused.files", { limit: number(REFUSE_FILES) })
              : t("tokens.folder.refused.bytes")}
          </p>
          <p class="note">{t("tokens.folder.refused.hint")}</p>
        </div>
        <footer>
          <button data-initial type="button" class="btn btn--primary" onclick={no}>
            {t("common.close")}
          </button>
        </footer>
      {/if}
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
    width: min(480px, 100%);
    max-height: 100%;
    overflow-y: auto;
    padding: var(--sp-5);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--bg);
    color: var(--text);
    box-shadow: var(--shadow-pop);
    outline: none;
  }
  h2 {
    margin: 0 0 var(--sp-3);
    font-size: 15px;
    font-weight: 600;
  }
  p {
    margin: 0 0 var(--sp-3);
  }
  .note {
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    gap: var(--sp-2);
    margin-top: var(--sp-4);
  }
</style>
