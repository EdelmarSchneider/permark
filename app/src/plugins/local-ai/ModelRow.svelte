<script lang="ts">
  // One model of the Models card. Line 1: name, size, terms (opened in the browser, never in the app); line 2: what it
  // can do, its languages, how it runs here (words + icon, never color alone). Then what its state allows: Get (or
  // the terms first, inline), progress + Cancel, Paused + Resume | Delete partial, the failure + Try again, Ready +
  // Remove (inline confirm naming the jobs that lose it), Update. Details: format, revision, source, who may use it.
  import CircleCheck from "@lucide/svelte/icons/circle-check";
  import CircleQuestionMark from "@lucide/svelte/icons/circle-question-mark";
  import CircleX from "@lucide/svelte/icons/circle-x";
  import Snail from "@lucide/svelte/icons/snail";
  import { tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { api, type Result } from "../../api";
  import type { CatalogModel } from "../../api/generated/CatalogModel";
  import type { LocalAiState } from "../../api/generated/LocalAiState";
  import type { ModelFit } from "../../api/generated/ModelFit";
  import type { ModelStatus } from "../../api/generated/ModelStatus";
  import { apiErrorText } from "../../core/errors";
  import { language, t, tn } from "../../core/i18n";
  import { localAi } from "./store.svelte";
  import type { AiTask } from "./tasks";
  import {
    capabilityKey,
    downloadErrorText,
    errorText,
    fitKey,
    languagesText,
    percentText,
    sizeText,
    taskKey,
  } from "./words";

  type Props = { model: CatalogModel; status: ModelStatus };
  let { model, status }: Props = $props();

  /** Why Get | Resume | Update cannot start now (`null`: they can). */
  type Hold =
    | { kind: "store" }
    | { kind: "withdrawn" }
    | { kind: "token" }
    | { kind: "noSpace"; needed: number; free: number };
  /** The inline panel open under the row. */
  type Panel = { kind: "none" } | { kind: "terms"; agreed: boolean } | { kind: "remove" };

  const id = $props.id();
  let row: HTMLLIElement | undefined = $state();
  let panel = $state<Panel>({ kind: "none" });
  let failure = $state<string | null>(null);

  const name = $derived(model.name);
  const size = $derived(sizeText(model.bytes));
  const fit = $derived<ModelFit | null>(localAi.machine?.fits[model.id] ?? null);
  const free = $derived(localAi.state?.store.freeBytes ?? null);
  /** Jobs that use this model now (they lose it when it is removed). */
  const jobs = $derived<AiTask[]>(
    (localAi.state?.assignments ?? []).flatMap((a) => (a.model === model.id ? [a.task] : [])),
  );
  const abilities = $derived(model.capabilities.map((c) => t(capabilityKey(c))).join(" · "));
  /** Bytes still to come for a Get | Resume. */
  const remaining = $derived(
    status.state.kind === "paused" ? status.state.total - status.state.done : model.bytes,
  );
  const hold = $derived.by((): Hold | null => {
    if (status.state.kind === "storeUnavailable") return { kind: "store" };
    if (status.blocked === "withdrawn") return { kind: "withdrawn" };
    if (status.blocked === "token") return { kind: "token" };
    if (free !== null && remaining > free) return { kind: "noSpace", needed: remaining, free };
    return null;
  });
  /** Get first shows the terms. */
  const termsFirst = $derived(status.blocked === "license");

  function holdText(hold: Hold): string {
    switch (hold.kind) {
      case "store":
        return t("localAi.models.storeUnavailable");
      case "withdrawn":
        return t("localAi.err.withdrawn");
      case "token":
        return t("localAi.models.blocked.token");
      case "noSpace":
        return t("localAi.models.blocked.noSpace", {
          needed: sizeText(hold.needed),
          free: sizeText(hold.free),
        });
      default:
        return hold satisfies never;
    }
  }

  function fitIcon(fit: ModelFit): typeof CircleCheck {
    switch (fit) {
      case "runsWell":
        return CircleCheck;
      case "slowCpuOnly":
        return Snail;
      case "notRecommended":
        return CircleX;
      case "unknown":
        return CircleQuestionMark;
      default:
        return fit satisfies never;
    }
  }

  function jobsText(tasks: readonly AiTask[]): string {
    const names = tasks.map((task) => t(taskKey(task)));
    return new Intl.ListFormat(language.locale, { type: "conjunction" }).format(names);
  }

  /** Focus on a named action of this row, else its first usable one, else Details (after the row is redrawn). */
  async function focusAction(action: string | null): Promise<void> {
    await tick();
    const root = row;
    if (root === undefined) return;
    const named =
      action === null
        ? null
        : root.querySelector<HTMLElement>(`[data-action="${action}"]:not(:disabled)`);
    const target =
      named ??
      root.querySelector<HTMLElement>(".actions button:not(:disabled)") ??
      root.querySelector<HTMLElement>("summary");
    target?.focus();
  }

  async function act(run: () => Promise<Result<LocalAiState>>): Promise<boolean> {
    const result = await run();
    failure = result.ok ? null : errorText(result.error);
    return result.ok;
  }

  async function openPanel(next: Panel, first: string): Promise<void> {
    panel = next;
    await tick();
    row?.querySelector<HTMLElement>(`[data-panel-first="${first}"]`)?.focus();
  }

  function closePanel(opener: string): void {
    panel = { kind: "none" };
    void focusAction(opener);
  }

  /** Escape inside a panel closes it (the settings page then stays open: the event is taken). */
  function escapeTo(opener: string): Attachment<HTMLElement> {
    return (panel) => {
      const listen = (e: KeyboardEvent): void => {
        if (e.key !== "Escape" || e.defaultPrevented) return;
        e.preventDefault();
        closePanel(opener);
      };
      panel.addEventListener("keydown", listen);
      return () => panel.removeEventListener("keydown", listen);
    };
  }

  /** Get | Resume | Try again: the terms first when they are not accepted yet. */
  function start(): void {
    if (termsFirst) void openPanel({ kind: "terms", agreed: false }, "terms");
    else void act(() => localAi.get(model.id));
  }

  async function acceptAndGet(): Promise<void> {
    if (!(await act(() => localAi.acceptLicense(model.id)))) return;
    if (!(await act(() => localAi.get(model.id)))) return;
    panel = { kind: "none" };
    void focusAction(null);
  }

  async function remove(): Promise<void> {
    const removed = await act(() => localAi.remove(model.id));
    panel = { kind: "none" };
    void focusAction(removed ? null : "remove");
  }

  async function openTerms(e: MouseEvent): Promise<void> {
    e.preventDefault();
    const result = await api.openWeb(model.license.url);
    failure = result.ok ? null : apiErrorText(result.error);
  }
</script>

<li class="model" data-model={model.id} bind:this={row}>
  <div class="head">
    <div class="text">
      <p class="line">
        <span class="name">{name}</span>
        <span class="quiet">{size}</span>
        <a
          class="license"
          href={model.license.url}
          aria-label={t("localAi.models.licenseLabel", { license: model.license.name, name })}
          onclick={(e) => void openTerms(e)}
          onauxclick={(e) => e.preventDefault()}>{model.license.name}</a
        >
      </p>
      <p class="line quiet">
        <span>{abilities}</span>
        <span>{t("localAi.models.languages", { list: languagesText(model.languages) })}</span>
      </p>
      {#if fit !== null}
        {@const Icon = fitIcon(fit)}
        <p class="fit fit--{fit}">
          <Icon size={14} aria-hidden="true" />
          <span>{t(fitKey(fit))}</span>
        </p>
      {/if}
    </div>

    <div class="actions">
      {#if status.state.kind === "notInstalled" || status.state.kind === "storeUnavailable"}
        {#if termsFirst && hold === null}
          <button
            type="button"
            class="btn btn--sm"
            data-action="terms"
            aria-expanded={panel.kind === "terms"}
            aria-controls={`${id}-terms`}
            aria-label={t("localAi.models.reviewTermsLabel", { name })}
            onclick={start}>{t("localAi.models.reviewTerms")}</button
          >
        {:else}
          <button
            type="button"
            class="btn btn--sm"
            data-action="get"
            disabled={hold !== null}
            aria-describedby={hold === null ? undefined : `${id}-hold`}
            aria-label={t("localAi.models.getLabel", { name, size })}
            onclick={start}>{t("localAi.models.get", { size })}</button
          >
        {/if}
      {:else if status.state.kind === "queued"}
        <span class="state">{t("localAi.dl.waiting")}</span>
        <button
          type="button"
          class="btn btn--sm btn--ghost"
          data-action="cancel"
          aria-label={t("localAi.dl.cancelLabel", { name })}
          onclick={() => void act(() => localAi.cancel(model.id))}>{t("common.cancel")}</button
        >
      {:else if status.state.kind === "downloading" || status.state.kind === "verifying"}
        {@const { done, total } = status.state}
        {@const verifying = status.state.kind === "verifying"}
        {@const amount = t("localAi.dl.progress", { done: sizeText(done), total: sizeText(total) })}
        <div class="progress">
          <span class="state">
            {verifying ? t("localAi.dl.verifying") : `${amount} (${percentText(done, total)})`}
          </span>
          <div
            class="bar"
            role="progressbar"
            aria-label={t(verifying ? "localAi.dl.verifyLabel" : "localAi.dl.barLabel", { name })}
            aria-valuemin={0}
            aria-valuemax={100}
            aria-valuenow={total > 0 ? Math.floor((done / total) * 100) : 0}
            aria-valuetext={verifying ? t("localAi.dl.verifying") : amount}
          >
            <div class="fill" style:width="{total > 0 ? (done / total) * 100 : 0}%"></div>
          </div>
        </div>
        {#if !verifying}
          <button
            type="button"
            class="btn btn--sm btn--ghost"
            data-action="cancel"
            aria-label={t("localAi.dl.cancelLabel", { name })}
            onclick={() => void act(() => localAi.cancel(model.id))}>{t("common.cancel")}</button
          >
        {/if}
      {:else if status.state.kind === "paused"}
        <span class="state">
          {t("localAi.dl.paused", { percent: percentText(status.state.done, status.state.total) })}
        </span>
        <button
          type="button"
          class="btn btn--sm"
          data-action="get"
          disabled={hold !== null}
          aria-describedby={hold === null ? undefined : `${id}-hold`}
          aria-label={t("localAi.dl.resumeLabel", { name })}
          onclick={start}>{t("localAi.dl.resume")}</button
        >
        <button
          type="button"
          class="btn btn--sm btn--ghost"
          data-action="deletePart"
          aria-label={t("localAi.dl.deletePartialLabel", { name })}
          onclick={() => void act(() => localAi.deletePart(model.id)).then(() => focusAction(null))}
          >{t("localAi.dl.deletePartial")}</button
        >
      {:else if status.state.kind === "failed"}
        <button
          type="button"
          class="btn btn--sm"
          data-action="get"
          disabled={hold !== null}
          aria-describedby={hold === null ? undefined : `${id}-hold`}
          aria-label={t("localAi.dl.retryLabel", { name })}
          onclick={start}>{t("localAi.retry")}</button
        >
      {:else if status.state.kind === "installed" || status.state.kind === "updateAvailable"}
        {#if status.state.kind === "updateAvailable"}
          <span class="state">{t("localAi.models.updateAvailable")}</span>
          <button
            type="button"
            class="btn btn--sm"
            data-action="update"
            disabled={hold !== null}
            aria-describedby={hold === null ? undefined : `${id}-hold`}
            aria-label={t("localAi.models.updateLabel", { name, size })}
            onclick={() => void act(() => localAi.update(model.id))}
            >{t("localAi.models.update", { size })}</button
          >
        {:else}
          <span class="state state--ready">
            <CircleCheck size={14} aria-hidden="true" />
            {t("localAi.models.ready")}
          </span>
        {/if}
        <button
          type="button"
          class="btn btn--sm btn--ghost"
          data-action="remove"
          aria-expanded={panel.kind === "remove"}
          aria-controls={`${id}-remove`}
          aria-label={t("localAi.models.removeLabel", { name })}
          onclick={() => void openPanel({ kind: "remove" }, "remove")}
          >{t("localAi.models.remove")}</button
        >
      {/if}
    </div>
  </div>

  {#if hold !== null}
    <p class="hold" id={`${id}-hold`}>{holdText(hold)}</p>
  {/if}
  {#if status.state.kind === "failed"}
    <p class="problem" role="alert">{downloadErrorText(status.state.error)}</p>
  {/if}
  {#if failure !== null}
    <p class="problem" role="alert">{failure}</p>
  {/if}

  {#if panel.kind === "terms"}
    {@const agreed = panel.agreed}
    <div
      class="panel"
      id={`${id}-terms`}
      role="group"
      aria-labelledby={`${id}-terms-title`}
      {@attach escapeTo("terms")}
    >
      <p class="panel-title" id={`${id}-terms-title`}>
        {t("localAi.terms.title", { name, license: model.license.name })}
      </p>
      {#if model.license.restriction !== null}
        <p class="quiet">
          {t("localAi.terms.restriction", { note: model.license.restriction })}
        </p>
      {/if}
      <p>
        <a
          href={model.license.url}
          onclick={(e) => void openTerms(e)}
          onauxclick={(e) => e.preventDefault()}>{t("localAi.terms.read")}</a
        >
      </p>
      <label class="agree">
        <input
          type="checkbox"
          data-panel-first="terms"
          checked={agreed}
          onchange={(e) => (panel = { kind: "terms", agreed: e.currentTarget.checked })}
        />
        <span>{t("localAi.terms.accept")}</span>
      </label>
      <div class="buttons">
        <button
          type="button"
          class="btn btn--sm btn--primary"
          data-action="acceptAndGet"
          disabled={!agreed}
          aria-label={t("localAi.terms.acceptAndGetLabel", { name })}
          onclick={() => void acceptAndGet()}>{t("localAi.terms.acceptAndGet")}</button
        >
        <button type="button" class="btn btn--sm btn--ghost" onclick={() => closePanel("terms")}
          >{t("common.cancel")}</button
        >
      </div>
    </div>
  {:else if panel.kind === "remove"}
    <div
      class="panel"
      id={`${id}-remove`}
      role="group"
      aria-label={t("localAi.models.removeLabel", { name })}
      {@attach escapeTo("remove")}
    >
      <p>
        {t("localAi.remove.confirm", { name, size })}
        {#if jobs.length > 0}
          {tn("localAi.remove.jobs", jobs.length, { jobs: jobsText(jobs) })}
        {/if}
      </p>
      <div class="buttons">
        <button
          type="button"
          class="btn btn--sm"
          data-action="confirmRemove"
          aria-label={t("localAi.models.removeLabel", { name })}
          onclick={() => void remove()}>{t("localAi.models.remove")}</button
        >
        <button
          type="button"
          class="btn btn--sm btn--ghost"
          data-panel-first="remove"
          onclick={() => closePanel("remove")}>{t("common.cancel")}</button
        >
      </div>
    </div>
  {/if}

  <details class="details">
    <summary aria-label={t("localAi.models.detailsLabel", { name })}>{t("common.details")}</summary>
    <dl>
      <dt>{t("localAi.models.details.format")}</dt>
      <dd>{model.quant}</dd>
      <dt>{t("localAi.models.details.revision")}</dt>
      <dd><code>{model.commit.slice(0, 7)}</code></dd>
      <dt>{t("localAi.models.details.source")}</dt>
      <dd>{model.repo}</dd>
      <dt>{t("localAi.models.details.size")}</dt>
      <dd>{size}</dd>
      {#if model.license.restriction !== null}
        <dt>{t("localAi.models.details.restriction")}</dt>
        <dd>{model.license.restriction}</dd>
      {/if}
    </dl>
  </details>
</li>

<style>
  .model {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-4);
    font-size: var(--fs-ui-sm);
  }
  p {
    margin: 0;
  }
  .head {
    display: flex;
    flex-wrap: wrap;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--sp-2) var(--sp-5);
  }
  .text {
    display: flex;
    flex: 1 1 16rem;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: baseline;
    gap: 0 var(--sp-2);
    overflow-wrap: anywhere;
  }
  .name {
    min-width: 0;
    color: var(--text);
    font-weight: 600;
    overflow-wrap: anywhere;
  }
  .quiet {
    color: var(--muted);
  }
  .license {
    color: var(--accent);
  }
  .fit {
    display: flex;
    align-items: center;
    gap: var(--sp-1);
  }
  .fit :global(svg),
  .state--ready :global(svg) {
    flex: none;
  }
  .fit--runsWell,
  .state--ready {
    color: var(--git-added-text);
  }
  .fit--slowCpuOnly {
    color: var(--git-modified-text);
  }
  .fit--notRecommended {
    color: var(--git-deleted-text);
  }
  .fit--unknown {
    color: var(--muted);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: var(--sp-2);
  }
  .state {
    display: inline-flex;
    align-items: center;
    gap: var(--sp-1);
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
  .progress {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    width: 12rem;
  }
  .bar {
    height: 4px;
    overflow: hidden;
    border-radius: 2px;
    background: var(--hover);
  }
  .fill {
    height: 100%;
    border-radius: 2px;
    background: var(--accent);
  }
  @media (prefers-reduced-motion: no-preference) {
    .fill {
      transition: width var(--dur-fast) linear;
    }
  }
  .hold {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .problem {
    color: var(--git-deleted-text);
    overflow-wrap: anywhere;
  }
  .panel {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-card);
    background: var(--hover);
    overflow-wrap: anywhere;
  }
  .panel-title {
    font-weight: 600;
  }
  .panel a {
    color: var(--accent);
  }
  .agree {
    display: flex;
    align-items: flex-start;
    gap: var(--sp-2);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }
  .details summary {
    width: fit-content;
    color: var(--muted);
    cursor: pointer;
  }
  .details dl {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr);
    gap: 2px var(--sp-3);
    margin: var(--sp-2) 0 0;
  }
  .details dt {
    color: var(--muted);
  }
  .details dd {
    margin: 0;
    overflow-wrap: anywhere;
  }
</style>
