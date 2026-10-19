<script lang="ts">
  // Status bar item beside the word count: the tokens of the document (or of the selection) by the first tokenizer
  // chosen, with its name. A click opens the popover: every tokenizer chosen, the document, the part without front
  // matter, the selection, and the choice itself. Nothing chosen: a chip that opens the choice. An estimate is always
  // marked (`≈`, "Approximate"); a count of the model's own tokenizer is not.
  import Hash from "@lucide/svelte/icons/hash";
  import { createAnalysisText } from "../../core/doc/analysisText.svelte";
  import { language, t, tn } from "../../core/i18n";
  import { createFigures } from "./figures.svelte";
  import { displayOf, failureKey, formatMegabytes, percentOf, type Figure } from "./model";
  import TokenChoice from "./TokenChoice.svelte";
  import { tokens } from "./store.svelte";
  import type { TokenizerInfo } from "./tokenizers";

  const source = createAnalysisText();
  const figures = createFigures(source);
  let box: HTMLElement | undefined = $state();

  const number = (n: number): string => new Intl.NumberFormat(language.locale).format(n);
  const mark = (figure: Figure): string => (figure.approx ? "≈ " : "");

  const primary = $derived(tokens.primary);
  const display = $derived.by(() => {
    const whole = primary === null ? null : figures.whole(primary);
    return displayOf({
      countable: source.countable,
      primary,
      vocab: primary === null ? null : tokens.vocabOf(primary.id),
      whole: whole ?? { value: 0, approx: true },
      selection: primary === null ? null : figures.selection(primary),
    });
  });
  const shown = $derived(display.kind !== "none" && (display.kind !== "count" || hasNumber()));

  function hasNumber(): boolean {
    return primary !== null && figures.whole(primary) !== null;
  }

  /** `1 234 tokens` | `120 of 1 234 tokens` of a pair of figures. */
  function phrase(whole: Figure, part: Figure | null): string {
    if (part !== null) {
      return t("tokens.countOf", { part: number(part.value), total: number(whole.value) });
    }
    return tn("tokens.count", whole.value, { n: number(whole.value) });
  }

  const label = $derived.by((): string => {
    switch (display.kind) {
      case "none":
        return "";
      case "choose":
        return t("tokens.choose");
      case "download":
        return t("tokens.download", { tokenizer: display.tokenizer.short });
      case "downloading":
        return t("tokens.downloading", {
          tokenizer: display.tokenizer.short,
          percent: display.percent,
        });
      case "failed":
        return t("tokens.unavailable", { tokenizer: display.tokenizer.short });
      case "count":
        return `${mark(display.whole)}${phrase(display.whole, display.selection)} · ${display.tokenizer.short}`;
      default:
        return display satisfies never;
    }
  });

  const tooltip = $derived.by((): string => {
    if (display.kind === "count") {
      const kind =
        display.tokenizer.exact && !display.whole.approx ? "" : ` — ${t("tokens.approxWord")}`;
      return `${label}${kind}`;
    }
    return label;
  });

  /** The status bar clips what overflows it: the popover is fixed, above the item, inside the window. */
  function above(node: HTMLElement): void {
    if (!box) return;
    const anchor = box.getBoundingClientRect();
    const left = Math.min(anchor.left, window.innerWidth - node.offsetWidth - 8);
    node.style.left = `${String(Math.max(8, left))}px`;
    node.style.bottom = `${String(window.innerHeight - anchor.top + 6)}px`;
  }

  // closes on Escape | a click outside
  $effect(() => {
    if (!tokens.popover) return;
    const close = (): void => {
      tokens.popover = false;
      tokens.choosing = false;
    };
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") close();
    };
    const onPointer = (e: PointerEvent): void => {
      if (box && e.target instanceof Node && !box.contains(e.target)) close();
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer);
    };
  });

  function toggle(): void {
    if (tokens.popover) {
      tokens.popover = false;
      tokens.choosing = false;
    } else if (display.kind === "choose" || tokens.chosen.length === 0) {
      tokens.openChooser();
    } else {
      tokens.popover = true;
    }
  }

  /** The row of a tokenizer in the popover: its figures, or why there are none. */
  function rowState(info: TokenizerInfo): "figures" | "download" | "downloading" | "failed" {
    if (!info.exact) return "figures";
    const vocab = tokens.vocabOf(info.id);
    if (vocab === null) return "figures";
    switch (vocab.state.kind) {
      case "ready":
        return "figures";
      case "absent":
        return "download";
      case "downloading":
        return "downloading";
      case "failed":
        return "failed";
      default:
        return vocab.state satisfies never;
    }
  }
</script>

{#if source.countable}
  <span class="token" bind:this={box}>
    {#if shown}
      <button
        type="button"
        class="item"
        class:is-chip={display.kind !== "count"}
        aria-haspopup="dialog"
        aria-expanded={tokens.popover}
        title={tooltip}
        onclick={toggle}
      >
        {#if display.kind !== "count"}<Hash size={13} aria-hidden="true" />{/if}
        <span class="label">{label}</span>
      </button>
    {/if}
    {#if tokens.popover}
      <div class="popover" role="dialog" aria-label={t("tokens.popover.title")} use:above>
        <p class="title">{t("tokens.popover.title")}</p>
        {#each tokens.chosen as info, at (info.id)}
          {@const state = rowState(info)}
          <div class="tokenizer">
            <p class="head">
              <span class="short">{info.short}</span>
              <span class="badge" class:is-exact={info.exact}>
                {info.exact ? t("tokens.badge.exact") : t("tokens.badge.approx")}
              </span>
              {#if at === 0 && tokens.chosen.length > 1}
                <span class="primary">{t("tokens.primary")}</span>
              {/if}
            </p>
            {#if state === "figures"}
              {@const whole = figures.whole(info)}
              {@const body = figures.body(info)}
              {@const part = figures.selection(info)}
              <dl class="figures">
                <dt>{t("tokens.row.document")}</dt>
                <dd>{whole === null ? "…" : `${mark(whole)}${number(whole.value)}`}</dd>
                {#if body !== null}
                  <dt>{t("tokens.row.body")}</dt>
                  <dd>{mark(body)}{number(body.value)}</dd>
                {/if}
                {#if part !== null}
                  <dt>{t("tokens.row.selection")}</dt>
                  <dd>{mark(part)}{number(part.value)}</dd>
                {/if}
              </dl>
            {:else}
              {@const vocab = tokens.vocabOf(info.id)}
              <p class="pending">
                {#if state === "download" && vocab !== null}
                  {t("tokens.download", { tokenizer: info.short })} ({formatMegabytes(
                    vocab.bytes,
                    language.locale,
                  )})
                {:else if state === "downloading"}
                  {t("tokens.downloading", {
                    tokenizer: info.short,
                    percent:
                      vocab?.state.kind === "downloading"
                        ? percentOf(vocab.state.done, vocab.state.total)
                        : 0,
                  })}
                {:else if vocab?.state.kind === "failed"}
                  {t(failureKey(vocab.state.reason), {
                    status: vocab.state.reason.kind === "http" ? vocab.state.reason.status : 0,
                  })}
                {/if}
              </p>
            {/if}
          </div>
        {/each}
        {#if tokens.chosen.length > 0}
          <p class="explain">{t("tokens.explain")}</p>
          <label class="switch">
            <input
              type="checkbox"
              checked={tokens.state?.outline ?? false}
              onchange={(e) => void tokens.setOutline(e.currentTarget.checked)}
            />
            <span>{t("tokens.outlineSwitch")}</span>
          </label>
        {/if}
        {#if tokens.choosing || tokens.chosen.length === 0}
          <div class="choice">
            <p class="subtitle">{t("tokens.change")}</p>
            <TokenChoice mode="both" />
          </div>
        {:else}
          <button type="button" class="btn btn--sm" onclick={() => (tokens.choosing = true)}>
            {t("tokens.change")}
          </button>
        {/if}
      </div>
    {/if}
  </span>
{/if}

<style>
  .token {
    position: relative;
    display: inline-flex;
    align-items: center;
    margin-right: var(--sp-3);
  }
  .item {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 6px;
    border: none;
    border-radius: 4px;
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
    font-variant-numeric: tabular-nums;
  }
  .item.is-chip {
    color: var(--accent);
  }
  .item:hover,
  .item[aria-expanded="true"] {
    background: var(--hover);
  }
  .label {
    white-space: nowrap;
  }
  .popover {
    position: fixed;
    z-index: 40;
    width: 380px;
    max-width: calc(100vw - 24px);
    max-height: calc(100vh - 80px);
    padding: var(--sp-3);
    overflow-y: auto;
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
  .subtitle {
    margin: 0 0 var(--sp-1);
    color: var(--muted);
    font-weight: 600;
  }
  .tokenizer + .tokenizer {
    margin-top: var(--sp-2);
    padding-top: var(--sp-2);
    border-top: 1px solid var(--border);
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
    margin: 0 0 2px;
  }
  .short {
    font-weight: 600;
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
  .figures {
    display: grid;
    grid-template-columns: 1fr auto;
    gap: 1px var(--sp-4);
    margin: 0;
    font-variant-numeric: tabular-nums;
  }
  .figures dt {
    color: var(--muted);
  }
  .figures dd {
    margin: 0;
    text-align: right;
  }
  .pending {
    margin: 0;
    color: var(--muted);
  }
  .explain {
    margin: var(--sp-3) 0 var(--sp-2);
    color: var(--muted);
  }
  .switch {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    margin-bottom: var(--sp-3);
    cursor: pointer;
  }
  .choice {
    padding-top: var(--sp-2);
    border-top: 1px solid var(--border);
  }
</style>
