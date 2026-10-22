<script lang="ts">
  // The quiet question under the Explorer list (the code offer's place) about projects in this folder: the first
  // question, new projects (where they are), "is this the missing one?", or that the project list can't be read.
  // Never a dialog, never announced; after an answer the focus goes back to the list (the move question: to the node
  // of the folder it was about).
  import { tick } from "svelte";
  import { language, t, tn } from "../i18n";
  import { projects } from "./instance.svelte";
  import { joinNames, newWhere, recordPlace } from "./model";

  type Props = {
    /** Shows and focuses the node of a folder in the list (the move question's folder after the answer). */
    focusNode?: (path: string) => void;
  };
  let { focusNode }: Props = $props();

  const strip = $derived(projects.strip);
  let busy = $state(false);

  async function answer(run: () => Promise<unknown>): Promise<void> {
    if (busy) return;
    busy = true;
    let done: unknown;
    try {
      done = await run();
    } finally {
      busy = false;
    }
    await tick();
    if (typeof done === "string" && focusNode) focusNode(done);
    else document.querySelector<HTMLElement>('[role="tree"]')?.focus();
  }

  function found(count: number, names: readonly string[]): string {
    return count <= 3 && names.length === count
      ? tn("projects.strip.foundNamed", count, { names: joinNames(names, language.locale) })
      : t("projects.strip.foundCount", { n: count });
  }

  /** Where the new projects are: their paths (up to 3), else a count in the folder holding most of them. */
  function where(count: number, paths: readonly string[]): string {
    const at = newWhere(paths, count);
    switch (at.kind) {
      case "one":
        return t("projects.strip.newOne", { path: at.path });
      case "few":
        return t("projects.strip.newFew", { paths: joinNames(at.paths, language.locale) });
      case "many": {
        const folder = at.folder === "" ? t("projects.strip.thisFolder") : `${at.folder}/`;
        return at.others === 0
          ? t("projects.strip.newIn", { n: at.count, folder })
          : tn("projects.strip.newInMore", at.others, { n: at.count, k: at.others, folder });
      }
      default:
        return at satisfies never;
    }
  }
</script>

{#if strip.kind !== "none"}
  <div class="strip" role="region" aria-label={t("projects.strip.label")} data-projects-strip>
    {#if strip.kind === "broken"}
      <p>{t("projects.strip.broken")}</p>
      <div class="buttons">
        <button type="button" class="btn" onclick={() => projects.showProblem()}
          >{t("projects.strip.showProblem")}</button
        >
      </div>
    {:else if strip.kind === "move"}
      <p>
        {t("projects.strip.isMoved", {
          folder: strip.ask.candidate,
          name: strip.ask.name,
          path: strip.ask.was,
        })}
      </p>
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
        <p>{found(strip.count, strip.names)} {tn("projects.strip.why", strip.count)}</p>
      {:else}
        <p>{where(strip.count, strip.paths)}</p>
      {/if}
      {#each strip.inner as line (line.name)}
        <p>{tn("projects.strip.inner", line.count, { name: line.name })}</p>
      {/each}
      <!-- before the answer: where it is saved (a file in this folder) -->
      <p class="place">{t(recordPlace(projects.view))}</p>
      <div class="buttons">
        <button
          type="button"
          class="btn btn--primary"
          disabled={busy}
          onclick={() => void answer(() => projects.includeAll())}
        >
          {strip.count === 1 ? t("projects.strip.include") : t("projects.strip.includeAll")}
        </button>
        {#if strip.count > 1}
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
  .place {
    color: var(--muted);
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
