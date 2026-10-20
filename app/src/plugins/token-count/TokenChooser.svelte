<script lang="ts">
  // The list of tokenizers to choose from: each with a badge that says whether its number is exact or only an estimate,
  // one line on why, and, for an exact one, the download of its vocabulary (never started by choosing it: the person
  // presses Download). Up to three can be chosen; the first is the one in the status bar.
  import { language, t } from "../../core/i18n";
  import { failureKey, formatMegabytes, percentOf } from "./model";
  import { tokens } from "./store.svelte";
  import { MAX_TOKENIZERS, TOKENIZERS, type TokenizerInfo } from "./tokenizers";

  type Props = {
    /** The ids chosen, first = primary. */
    ids: readonly string[];
    onchange: (ids: string[]) => void;
    /** Show "Remove" for a downloaded vocabulary (the settings card; the popover keeps it short). */
    removable?: boolean;
  };
  let { ids, onchange, removable = false }: Props = $props();

  const full = $derived(ids.length >= MAX_TOKENIZERS);

  function toggle(info: TokenizerInfo, on: boolean): void {
    const rest = ids.filter((id) => id !== info.id);
    onchange(on ? [...rest, info.id] : rest);
  }

  function makePrimary(info: TokenizerInfo): void {
    onchange([info.id, ...ids.filter((id) => id !== info.id)]);
  }

  const size = (bytes: number): string => formatMegabytes(bytes, language.locale);
</script>

<ul class="chooser" aria-label={t("tokens.change")}>
  {#each TOKENIZERS as info (info.id)}
    {@const on = ids.includes(info.id)}
    {@const vocab = info.exact ? tokens.vocabOf(info.id) : null}
    <li class="row" class:is-on={on}>
      <label class="pick">
        <input
          type="checkbox"
          checked={on}
          disabled={!on && full}
          onchange={(e) => toggle(info, e.currentTarget.checked)}
        />
        <span class="names">
          <span class="line">
            <span class="short">{info.short}</span>
            <span class="badge" class:is-exact={info.exact}>
              {info.exact ? t("tokens.badge.exact") : t("tokens.badge.approx")}
            </span>
            {#if on && ids[0] === info.id}
              <span class="primary">{t("tokens.primary")}</span>
            {/if}
          </span>
          {#if t(info.label) !== info.short}
            <span class="models">{t(info.label)}</span>
          {/if}
          <span class="why">{t(info.why)}</span>
        </span>
      </label>
      <div class="actions">
        {#if on && ids[0] !== info.id}
          <button type="button" class="btn btn--sm btn--ghost" onclick={() => makePrimary(info)}>
            {t("tokens.makePrimary")}
          </button>
        {/if}
        {#if vocab !== null}
          {#if vocab.state.kind === "absent"}
            <button
              type="button"
              class="btn btn--sm"
              title={t("tokens.vocab.hint")}
              onclick={() => void tokens.download(info.id)}
            >
              {t("tokens.vocab.download", { size: size(vocab.bytes) })}
            </button>
          {:else if vocab.state.kind === "downloading"}
            <span class="progress" role="status">
              {t("tokens.vocab.progress", {
                percent: percentOf(vocab.state.done, vocab.state.total),
              })}
            </span>
            <button
              type="button"
              class="btn btn--sm btn--ghost"
              onclick={() => void tokens.cancel(info.id)}
            >
              {t("common.cancel")}
            </button>
          {:else if vocab.state.kind === "failed"}
            <span class="problem" role="alert">
              {t(failureKey(vocab.state.reason), {
                status: vocab.state.reason.kind === "http" ? vocab.state.reason.status : 0,
              })}
            </span>
            <button type="button" class="btn btn--sm" onclick={() => void tokens.download(info.id)}>
              {t("tokens.vocab.retry")}
            </button>
          {:else if removable}
            <span class="ready">{t("tokens.vocab.ready")}</span>
            <button
              type="button"
              class="btn btn--sm btn--ghost"
              onclick={() => void tokens.remove(info.id)}
            >
              {t("tokens.vocab.remove", { size: size(vocab.bytes) })}
            </button>
          {:else}
            <span class="ready">{t("tokens.vocab.ready")}</span>
          {/if}
        {/if}
      </div>
    </li>
  {/each}
</ul>
<p class="limit">{t("tokens.limit", { max: MAX_TOKENIZERS })}</p>

<style>
  .chooser {
    display: flex;
    flex-direction: column;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .row {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
    padding: var(--sp-2) 0;
  }
  .row + .row {
    border-top: 1px solid var(--border);
  }
  .pick {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
    flex: 1 1 14rem;
    min-width: 0;
    cursor: pointer;
  }
  .pick input {
    margin-top: 3px;
  }
  .names {
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
  }
  .short {
    font-weight: 600;
  }
  .models {
    color: var(--text);
    font-size: var(--fs-ui-sm);
  }
  .why {
    color: var(--muted);
    font-size: var(--fs-ui-xs);
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
  .primary {
    color: var(--accent-text);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-2);
    font-size: var(--fs-ui-sm);
  }
  .progress,
  .ready {
    color: var(--muted);
  }
  .problem {
    max-width: 22ch;
    color: var(--git-deleted-text);
    font-size: var(--fs-ui-xs);
  }
  .limit {
    margin: var(--sp-2) 0 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
</style>
