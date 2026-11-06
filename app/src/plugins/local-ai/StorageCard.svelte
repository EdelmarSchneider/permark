<script lang="ts">
  // "Storage": the folder the models live in (selectable text), Change (the system's folder picker), Show in folder,
  // Back to default; what the folder holds (used by N models, free on its drive, unfinished downloads). Models already
  // there: the change first asks inline whether they move along (progress + Cancel) or stay where they are. Refusals
  // (inside a project, inside the app's data, a download running...) and an unusable folder are said in words.
  import { tick } from "svelte";
  import type { Attachment } from "svelte/attachments";
  import { api } from "../../api";
  import type { InstallState } from "../../api/generated/InstallState";
  import type { StoreMode } from "../../api/generated/StoreMode";
  import type { StoreUnavailableReason } from "../../api/generated/StoreUnavailableReason";
  import { sizeText, t, tn } from "../../core/i18n";
  import Card from "../../core/shell/settings/Card.svelte";
  import SettingRow from "../../core/shell/settings/SettingRow.svelte";
  import { localAi } from "./store.svelte";
  import { asksMoveOrKeep, downloadErrorText, errorText, percentText } from "./words";

  /** The button that opened the question (focus goes back to it). */
  type Opener = "change" | "default";
  /** The inline question: move the installed models to `path` (`null`: the default folder) or keep them. */
  type Panel = { kind: "none" } | { kind: "ask"; path: string | null; opener: Opener };

  const RUNNING: ReadonlySet<InstallState["kind"]> = new Set([
    "queued",
    "downloading",
    "verifying",
  ]);

  const id = $props.id();
  let card: HTMLElement | undefined = $state();
  let panel = $state<Panel>({ kind: "none" });
  let failure = $state<string | null>(null);

  const store = $derived(localAi.state?.store ?? null);
  const move = $derived(localAi.state?.storeMove ?? null);
  const moving = $derived(move?.kind === "moving");
  const models = $derived(localAi.state?.models ?? []);
  const downloading = $derived(models.some((m) => RUNNING.has(m.state.kind)));
  const installedCount = $derived(models.filter((m) => m.installedVersion !== null).length);
  /** Change | Back to default wait for the download | move to end. */
  const locked = $derived(downloading || moving);

  const usage = $derived.by((): string => {
    if (store === null) return "";
    const parts = [
      installedCount === 0
        ? t("localAi.usage.none")
        : tn("localAi.usage.models", installedCount, { size: sizeText(store.usedBytes) }),
    ];
    if (store.freeBytes !== null)
      parts.push(t("localAi.usage.free", { size: sizeText(store.freeBytes) }));
    if (store.partialBytes > 0)
      parts.push(t("localAi.usage.partial", { size: sizeText(store.partialBytes) }));
    return parts.join(" · ");
  });

  function unavailableText(reason: StoreUnavailableReason | null): string {
    switch (reason) {
      case "missing":
      case null:
        return t("localAi.store.missing");
      case "insideProject":
        return t("localAi.store.insideProject");
      default:
        return reason satisfies never;
    }
  }

  /** Focus on a named button of the card, else Change (after the card is redrawn). */
  async function focusOn(action: string): Promise<void> {
    await tick();
    const root = card;
    if (root === undefined) return;
    const target =
      root.querySelector<HTMLElement>(`[data-action="${action}"]:not(:disabled)`) ??
      root.querySelector<HTMLElement>('[data-action="change"]:not(:disabled)') ??
      root.querySelector<HTMLElement>("button:not(:disabled)");
    target?.focus();
  }

  async function choose(path: string | null, opener: Opener): Promise<void> {
    const result = await localAi.setStore(path);
    if (result.ok) {
      failure = null;
      return;
    }
    if (asksMoveOrKeep(result.error)) {
      failure = null;
      panel = { kind: "ask", path, opener };
      await tick();
      card?.querySelector<HTMLElement>('[data-panel-first="ask"]')?.focus();
      return;
    }
    failure = errorText(result.error);
  }

  async function change(): Promise<void> {
    const picked = await api.pickFolder(t("localAi.store.pickTitle"), store?.path ?? null);
    if (picked !== null) await choose(picked, "change");
  }

  async function decide(mode: StoreMode): Promise<void> {
    if (panel.kind !== "ask") return;
    const { path, opener } = panel;
    const result = await localAi.setStore(path, mode);
    panel = { kind: "none" };
    failure = result.ok ? null : errorText(result.error);
    void focusOn(localAi.state?.storeMove?.kind === "moving" ? "cancelMove" : opener);
  }

  function closePanel(): void {
    if (panel.kind !== "ask") return;
    const { opener } = panel;
    panel = { kind: "none" };
    void focusOn(opener);
  }

  /** Escape inside the question closes it (the settings page then stays open: the event is taken). */
  const escapeCloses: Attachment<HTMLElement> = (element) => {
    const listen = (e: KeyboardEvent): void => {
      if (e.key !== "Escape" || e.defaultPrevented) return;
      e.preventDefault();
      closePanel();
    };
    element.addEventListener("keydown", listen);
    return () => element.removeEventListener("keydown", listen);
  };

  async function cancelMove(): Promise<void> {
    const result = await localAi.cancelMove();
    failure = result.ok ? null : errorText(result.error);
    void focusOn("change");
  }

  async function reveal(): Promise<void> {
    const result = await localAi.revealStore();
    failure = result.ok ? null : errorText(result.error);
  }
</script>

<section class="storage" aria-labelledby={`${id}-title`} bind:this={card}>
  <h3 id={`${id}-title`}>{t("localAi.store.title")}</h3>
  {#if store !== null}
    <Card>
      <SettingRow
        label={t("localAi.store.folder")}
        hint={store.isDefault ? t("localAi.store.default") : undefined}
        labelId={`${id}-folder`}
        wide
      >
        <div class="value">
          <p class="path">{store.path}</p>
          <div class="buttons">
            <button
              type="button"
              class="btn btn--sm"
              data-action="change"
              disabled={locked}
              aria-describedby={locked ? `${id}-locked` : undefined}
              aria-expanded={panel.kind === "ask" && panel.opener === "change"}
              aria-controls={`${id}-ask`}
              onclick={() => void change()}>{t("localAi.store.change")}</button
            >
            <button
              type="button"
              class="btn btn--sm btn--ghost"
              data-action="reveal"
              disabled={!store.available}
              onclick={() => void reveal()}>{t("localAi.store.reveal")}</button
            >
            {#if !store.isDefault}
              <button
                type="button"
                class="btn btn--sm btn--ghost"
                data-action="default"
                disabled={locked}
                aria-describedby={locked ? `${id}-locked` : undefined}
                aria-expanded={panel.kind === "ask" && panel.opener === "default"}
                aria-controls={`${id}-ask`}
                onclick={() => void choose(null, "default")}>{t("localAi.store.useDefault")}</button
              >
            {/if}
          </div>
        </div>
      </SettingRow>
      <div class="body">
        {#if store.available}
          <p class="quiet usage">{usage}</p>
        {:else}
          <p class="problem" role="alert">{unavailableText(store.unavailableReason)}</p>
        {/if}
        {#if locked}
          <p class="quiet" id={`${id}-locked`}>
            {moving ? t("localAi.store.movingHint") : t("localAi.store.downloadingHint")}
          </p>
        {/if}

        {#if move?.kind === "moving"}
          {@const amount = t("localAi.dl.progress", {
            done: sizeText(move.done),
            total: sizeText(move.total),
          })}
          <div class="moving">
            <div class="progress">
              <span class="state">
                {t("localAi.store.moving", { amount, percent: percentText(move.done, move.total) })}
              </span>
              <div
                class="bar"
                role="progressbar"
                aria-label={t("localAi.store.moveLabel")}
                aria-valuemin={0}
                aria-valuemax={100}
                aria-valuenow={move.total > 0 ? Math.floor((move.done / move.total) * 100) : 0}
                aria-valuetext={amount}
              >
                <div
                  class="fill"
                  style:width="{move.total > 0 ? (move.done / move.total) * 100 : 0}%"
                ></div>
              </div>
            </div>
            <button
              type="button"
              class="btn btn--sm btn--ghost"
              data-action="cancelMove"
              aria-label={t("localAi.store.cancelMoveLabel")}
              onclick={() => void cancelMove()}>{t("common.cancel")}</button
            >
          </div>
        {:else if move?.kind === "failed"}
          <p class="problem" role="alert">
            {t("localAi.store.moveFailed", { reason: downloadErrorText(move.error) })}
          </p>
        {/if}

        {#if panel.kind === "ask"}
          <div
            class="panel"
            id={`${id}-ask`}
            role="group"
            aria-labelledby={`${id}-ask-title`}
            {@attach escapeCloses}
          >
            <p id={`${id}-ask-title`}>
              {t("localAi.store.askMove", { size: sizeText(store.usedBytes) })}
            </p>
            <div class="buttons">
              <button
                type="button"
                class="btn btn--sm btn--primary"
                data-panel-first="ask"
                data-action="move"
                onclick={() => void decide("move")}>{t("localAi.store.move")}</button
              >
              <button
                type="button"
                class="btn btn--sm"
                data-action="keep"
                onclick={() => void decide("keep")}
                >{panel.path === null
                  ? t("localAi.store.keepDefault")
                  : t("localAi.store.keep")}</button
              >
              <button type="button" class="btn btn--sm btn--ghost" onclick={closePanel}
                >{t("common.cancel")}</button
              >
            </div>
          </div>
        {/if}

        {#if failure !== null}
          <p class="problem" role="alert">{failure}</p>
        {/if}
      </div>
    </Card>
  {/if}
</section>

<style>
  .storage {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  h3 {
    margin: 0;
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  p {
    margin: 0;
  }
  .value {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    min-width: 0;
  }
  .path {
    font-size: var(--fs-ui-sm);
    overflow-wrap: anywhere;
    user-select: text;
    -webkit-user-select: text;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-3) var(--sp-4);
    font-size: var(--fs-ui-sm);
  }
  .quiet {
    color: var(--muted);
    overflow-wrap: anywhere;
  }
  .usage {
    font-variant-numeric: tabular-nums;
  }
  .problem {
    color: var(--git-deleted-text);
    overflow-wrap: anywhere;
  }
  .moving {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2) var(--sp-3);
  }
  .progress {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    width: 16rem;
    max-width: 100%;
  }
  .state {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
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
</style>
