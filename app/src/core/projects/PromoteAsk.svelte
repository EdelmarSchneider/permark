<script lang="ts">
  // "Make official in {root}" for a local | unlisted tag | category: a first click asks in place, saying what it does
  // and which file it writes (`.permark/vocabulary.toml` in the opened folder, made | grown); "Make official" writes,
  // Cancel | Esc goes back to the button. While the list can't take a name, the button is off and says why.
  import { tick } from "svelte";
  import type { VocabularyKind } from "../../api";
  import { t } from "../i18n";
  import { projects } from "./instance.svelte";
  import { promoteBlocked, vocabularyPlace } from "./vocabulary";

  type Props = {
    kind: VocabularyKind;
    name: string;
    /** It is official now (focus goes where the caller wants it; the mark changed). */
    done?: () => void;
    /** A row of many (Tags tab): the button says "Make official" (its name adds the tag); the question takes the row's
     *  full width below. */
    short?: boolean;
  };
  let { kind, name, done, short = false }: Props = $props();

  let asking = $state(false);
  let busy = $state(false);
  let opener: HTMLButtonElement | undefined = $state();
  let confirm: HTMLButtonElement | undefined = $state();

  const view = $derived(projects.view);
  const blocked = $derived(promoteBlocked(view));
  const root = $derived(projects.rootName);

  async function ask(): Promise<void> {
    asking = true;
    await tick();
    confirm?.focus();
  }

  async function back(): Promise<void> {
    asking = false;
    await tick();
    opener?.focus();
  }

  async function go(): Promise<void> {
    if (busy) return;
    busy = true;
    const official = await projects.promote(kind, name);
    busy = false;
    asking = false;
    if (official) done?.();
    else {
      await tick();
      opener?.focus();
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key !== "Escape" || e.isComposing) return;
    e.preventDefault();
    e.stopPropagation();
    void back();
  }
</script>

{#if asking}
  <div
    class="ask"
    class:wide={short}
    role="group"
    aria-label={t(kind === "tag" ? "tags.promote" : "projects.category.promote", { root })}
  >
    <p>
      {t(kind === "tag" ? "tags.promoteAsk" : "projects.category.promoteAsk", { name, root })}
    </p>
    <p class="where">{t(vocabularyPlace(view))}</p>
    <div class="buttons">
      <button
        type="button"
        class="btn btn--sm btn--primary"
        bind:this={confirm}
        disabled={busy}
        {onkeydown}
        onclick={() => void go()}>{t("tags.promoteConfirm")}</button
      >
      <button
        type="button"
        class="btn btn--sm"
        disabled={busy}
        {onkeydown}
        onclick={() => void back()}>{t("common.cancel")}</button
      >
    </div>
  </div>
{:else}
  <button
    type="button"
    class="btn btn--sm promote"
    bind:this={opener}
    disabled={blocked !== null}
    title={blocked ?? (short ? t("tags.promote", { root }) : undefined)}
    aria-label={short ? t("tags.promoteRowNamed", { name }) : undefined}
    onclick={() => void ask()}
    >{short
      ? t("tags.promoteRow")
      : t(kind === "tag" ? "tags.promote" : "projects.category.promote", { root })}</button
  >
{/if}

<style>
  .ask {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    max-width: 100%;
    padding: var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface-2);
    font-size: var(--fs-ui-sm);
  }
  .wide {
    flex: 1 1 100%;
  }
  p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .where {
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    margin-top: var(--sp-1);
  }
  .promote {
    max-width: 100%;
    white-space: normal;
    text-align: left;
  }
</style>
