<script lang="ts">
  // The quiet question under the Explorer list (the code offer's place) about projects in this folder: the first
  // question, new projects, or "is this the missing one?". Never a dialog, never announced; after an answer the focus
  // goes back to the list.
  import { tick } from "svelte";
  import { language, t, tn } from "../i18n";
  import { projects } from "./instance.svelte";
  import { joinNames } from "./model";

  const strip = $derived(projects.strip);
  let busy = $state(false);

  async function answer(run: () => Promise<void> | void): Promise<void> {
    if (busy) return;
    busy = true;
    try {
      await run();
    } finally {
      busy = false;
    }
    await tick();
    document.querySelector<HTMLElement>('[role="tree"]')?.focus();
  }

  function found(count: number, names: readonly string[]): string {
    return count <= 3 && names.length === count
      ? tn("projects.strip.foundNamed", count, { names: joinNames(names, language.locale) })
      : t("projects.strip.foundCount", { n: count });
  }
</script>

{#if strip.kind !== "none"}
  <div class="strip" role="region" aria-label={t("projects.strip.label")}>
    {#if strip.kind === "move"}
      <p>{t("projects.strip.isMoved", { name: strip.ask.name, path: strip.ask.was })}</p>
      <div class="buttons">
        <button
          type="button"
          class="btn"
          disabled={busy}
          onclick={() => void answer(() => projects.answerMove(true))}
          >{t("projects.strip.yesMoved")}</button
        >
        <button
          type="button"
          class="btn"
          disabled={busy}
          onclick={() => void answer(() => projects.answerMove(false))}
          >{t("projects.strip.noNew")}</button
        >
      </div>
    {:else}
      {#if strip.kind === "first"}
        <p>{found(strip.count, strip.names)} {t("projects.strip.why")}</p>
      {:else}
        <p>
          {tn("projects.strip.new", strip.count, { name: strip.names[0] ?? "" })}
        </p>
      {/if}
      {#each strip.inner as line (line.name)}
        <p>{tn("projects.strip.inner", line.count, { name: line.name })}</p>
      {/each}
      <div class="buttons">
        <button
          type="button"
          class="btn btn--primary"
          disabled={busy}
          onclick={() => void answer(() => projects.includeAll())}
        >
          {strip.kind === "new" && strip.count === 1
            ? t("projects.strip.include")
            : t("projects.strip.includeAll")}
        </button>
        {#if strip.kind === "first" || strip.count > 1}
          <button type="button" class="btn" disabled={busy} onclick={() => projects.beginChoose()}
            >{t("projects.strip.choose")}</button
          >
        {/if}
        <button
          type="button"
          class="btn btn--ghost"
          disabled={busy}
          onclick={() => void answer(() => projects.declineOffered())}
          >{t("projects.strip.decline")}</button
        >
      </div>
    {/if}
  </div>
{/if}

<style>
  .strip {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
    padding: var(--sp-2) var(--sp-3);
    border-top: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    font-size: var(--fs-ui-sm);
    animation: pm-fade var(--dur-fast) var(--ease);
  }
  p {
    margin: 0;
    overflow-wrap: anywhere;
  }
  .buttons {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
  }
  @media (prefers-reduced-motion: reduce) {
    .strip {
      animation: none;
    }
  }
</style>
