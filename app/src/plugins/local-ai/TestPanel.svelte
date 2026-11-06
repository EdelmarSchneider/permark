<script lang="ts">
  // "Test model" under one model's row: where the test stands (getting the engine the first time, loading, running),
  // with progress where there is some and Cancel; then what it measured in plain words (where it runs, load time,
  // speed + the answer | ready to find by meaning, memory) + Close, or why it failed + Try again, or "stopped". The
  // headline is a polite live region (phase changes only, never every tick). Each button that appears takes the
  // keyboard when the one before it went away (never from another row).
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import type { Attachment } from "svelte/attachments";
  import type { CatalogModel } from "../../api/generated/CatalogModel";
  import { sizeText, t } from "../../core/i18n";
  import { localAi, type ModelTestView, type TestStep } from "./store.svelte";
  import { memoryText, percentText, secondsText, speedText, testErrorText } from "./words";

  type Props = { model: CatalogModel; view: ModelTestView; onclose: () => void };
  let { model, view, onclose }: Props = $props();

  const id = $props.id();
  const name = $derived(model.name);
  const step = $derived(view.step);
  const running = $derived(
    step.kind === "starting" ||
      step.kind === "engine" ||
      step.kind === "loading" ||
      step.kind === "running",
  );

  function headline(step: TestStep): string {
    switch (step.kind) {
      case "starting":
        return t("localAi.test.starting");
      case "engine":
        return t("localAi.test.engine");
      case "loading":
        return t("localAi.test.loading");
      case "running":
        return t("localAi.test.running");
      case "done":
        return t("localAi.test.done");
      case "failed":
        return t("localAi.test.failed");
      case "cancelled":
        return t("localAi.test.cancelled");
      default:
        return step satisfies never;
    }
  }

  /** Takes the keyboard when it is nowhere or still in this row (the button before it went away). */
  const takeFocus: Attachment<HTMLElement> = (button) => {
    const now = document.activeElement;
    const home = button.closest("[data-model]");
    if (now === null || now === document.body || (home !== null && home.contains(now)))
      button.focus();
  };
</script>

<div class="test" role="group" aria-label={t("localAi.test.panelLabel", { name })}>
  <div class="status" role="status" aria-live="polite" id={`${id}-status`}>
    {#if step.kind === "done"}
      <p class="title title--done">
        <CircleCheck size={14} aria-hidden="true" />
        <span>{headline(step)}</span>
      </p>
    {:else}
      <p class="title">{headline(step)}</p>
    {/if}
    {#if step.kind === "failed"}
      <p class="problem">{testErrorText(step.error)}</p>
    {/if}
  </div>

  {#if step.kind === "engine"}
    {@const { receivedBytes, totalBytes } = step}
    <div class="progress">
      <progress
        max={totalBytes > 0 ? totalBytes : 1}
        value={receivedBytes}
        aria-label={t("localAi.test.engine")}
      ></progress>
      <span class="quiet"
        >{t("localAi.dl.progress", {
          done: sizeText(receivedBytes),
          total: sizeText(totalBytes),
        })}</span
      >
    </div>
  {:else if step.kind === "loading"}
    <div class="progress">
      <progress max={1} value={step.progress} aria-label={t("localAi.test.loading")}></progress>
      <span class="quiet">{percentText(step.progress, 1)}</span>
    </div>
  {:else if step.kind === "done"}
    {@const { result } = step}
    <ul class="facts">
      <li>{t(result.gpu ? "localAi.test.gpu" : "localAi.test.cpu")}</li>
      <li>{t("localAi.test.loadTime", { time: secondsText(result.loadMs) })}</li>
      {#if result.output.kind === "chat"}
        <li>{speedText(result.output.tokensPerSecond)}</li>
      {:else}
        <li>{t("localAi.test.embedReady")}</li>
        <li>{t("localAi.test.embedTime", { time: secondsText(result.runMs) })}</li>
      {/if}
      {#if result.memoryBytes !== null}
        <li>{t("localAi.test.memory", { size: memoryText(result.memoryBytes) })}</li>
      {/if}
    </ul>
    {#if result.output.kind === "chat"}
      <p class="quiet">{t("localAi.test.answer")}</p>
      <blockquote class="answer">{result.output.answer}</blockquote>
    {/if}
  {/if}

  <div class="buttons">
    {#if running}
      <button
        type="button"
        class="btn btn--sm btn--ghost"
        data-action="cancelTest"
        aria-label={t("localAi.test.cancelLabel", { name })}
        aria-describedby={`${id}-status`}
        onclick={() => void localAi.cancelTest()}
        {@attach takeFocus}>{t("common.cancel")}</button
      >
    {:else}
      {#if step.kind === "failed"}
        <button
          type="button"
          class="btn btn--sm"
          data-action="retryTest"
          aria-label={t("localAi.test.retryLabel", { name })}
          onclick={() => void localAi.test(model.id)}
          {@attach takeFocus}>{t("localAi.retry")}</button
        >
      {/if}
      <button
        type="button"
        class="btn btn--sm btn--ghost"
        data-action="closeTest"
        aria-label={t("localAi.test.closeLabel", { name })}
        onclick={onclose}
        {@attach step.kind === "failed" ? undefined : takeFocus}>{t("common.close")}</button
      >
    {/if}
  </div>
</div>

<style>
  .test {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--hover);
    overflow-wrap: anywhere;
  }
  p {
    margin: 0;
  }
  .status {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
  }
  .title {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
    font-weight: 600;
  }
  .title :global(svg) {
    flex: none;
  }
  .title--done {
    color: var(--git-added-text);
  }
  .problem {
    color: var(--git-deleted-text);
  }
  .quiet {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .progress {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1) var(--sp-2);
  }
  progress {
    width: 12rem;
    max-width: 100%;
    accent-color: var(--accent);
  }
  .facts {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding-left: var(--sp-4);
  }
  .answer {
    margin: 0;
    padding: var(--sp-1) var(--sp-3);
    border-left: 2px solid var(--border);
    color: var(--text);
    white-space: pre-wrap;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }
</style>
