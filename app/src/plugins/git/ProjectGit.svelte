<script lang="ts">
  // The git block of a project's detail card (`projects.md` §12.4): branch (+ ahead | behind as of the last fetch),
  // changed files, last commit; "Checking git…" while the first look runs; git missing | can't read: one sentence.
  // Three lines are kept for it from the start, so the card grows at most once.
  import type { ProjectCardProps } from "../../core/plugins/registry.svelte";
  import { language, t } from "../../core/i18n";
  import { relativeTime } from "../../core/time";
  import {
    STATE_KEYS,
    aheadBehindText,
    changedLine,
    checking,
    gitMissing,
    headText,
    reasonText,
    stateHint,
  } from "./facts";
  import { members } from "./members.svelte";

  let { row }: ProjectCardProps = $props();

  const repo = $derived(members.of(row.uid));
  const hasGit = $derived(row.marker === "git" || row.marker === "both");
  const head = $derived(repo === null ? null : headText(repo));
  const state = $derived(repo === null ? null : STATE_KEYS[repo.state]);
  const hint = $derived(repo === null ? null : stateHint(repo.state));
  const ahead = $derived(repo?.aheadBehind ?? null);
  const commit = $derived(repo?.lastCommit ?? null);
  // the age is read when the card shows (a card stays open briefly)
  const age = $derived(
    commit === null ? "" : relativeTime(commit.atMs, Date.now(), language.locale),
  );
</script>

{#if hasGit}
  <div class="git" data-project-git>
    {#if repo === null || checking(repo)}
      <p class="muted">{t("projects.git.checking")}</p>
    {:else if gitMissing(repo)}
      <p class="muted">{t("projects.card.gitMissing")}</p>
    {:else if repo.error?.kind === "failed"}
      {#if head}<p>{t("projects.card.branch", { branch: head })}</p>{/if}
      <p>{t("projects.card.gitError")}</p>
      <p class="muted cut" title={repo.error.reason}>{reasonText(repo.error.reason)}</p>
    {:else}
      <p class="line">
        {#if head}
          <span class="cut" title={head}
            >{repo.detached ? head : t("projects.card.branch", { branch: head })}</span
          >
        {/if}
        {#if state}<span class="state" title={hint}
            >{t(state)}<span class="sr-only">, {hint}</span></span
          >{/if}
        {#if ahead !== null}
          {@const label = aheadBehindText(ahead[0], ahead[1])}
          <span class="ahead" role="img" aria-label={label} title={label}
            >{t("projects.card.aheadBehind", { a: ahead[0], b: ahead[1] })}</span
          >
        {/if}
      </p>
      {#if repo.changed !== null}<p>{changedLine(repo.changed)}</p>{/if}
      {#if repo.unborn}
        <p class="muted">{t("projects.card.noCommits")}</p>
      {:else if commit !== null}
        {@const text = t("projects.card.lastCommit", { age, subject: commit.subject })}
        <p class="muted cut" title={text}>{text}</p>
      {/if}
    {/if}
  </div>
{/if}

<style>
  .git {
    display: flex;
    flex-direction: column;
    gap: 2px;
    /* three lines kept from the start: the card grows at most once */
    min-height: calc(3 * 1.45em + 4px);
  }
  p {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .line {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
    min-width: 0;
  }
  .cut {
    min-width: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted {
    color: var(--muted);
  }
  .state {
    flex: none;
    padding: 0 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--git-modified) 18%, transparent);
    /* text color on the tint: AA in both themes (as the tree's chip) */
    color: var(--text);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  .ahead {
    flex: none;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }
</style>
