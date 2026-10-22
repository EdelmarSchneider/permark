<script lang="ts">
  // The git item of the status bar: branch, where HEAD is, how many files changed, and what the repository is doing
  // (merging, rebasing, ...). A click opens the legend: the letters in plain words, and what changes are counted from.
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import type { BaselineKind, Letter } from "../../api";
  import { t, type MessageKey } from "../../core/i18n";
  import { projects } from "../../core/projects/instance.svelte";
  import { STATE_KEYS, changedCount, gitMissing, shortOid as cut } from "./facts";
  import { members } from "./members.svelte";
  import { LETTER_ORDER, git, letterOf } from "./store.svelte";

  /** What the item shows: the repository of the active document's project (with its short name before it), else the
   *  opened folder's. */
  type Shown = {
    prefix: string | null;
    branch: string;
    oid: string | null;
    detached: boolean;
    unborn: boolean;
    changed: string;
    state: MessageKey | null;
    problem: string | null;
    baseline: BaselineKind;
    backend: "system" | "builtin" | null;
  };

  const snapshot = $derived(git.snapshot);
  const shown = $derived.by((): Shown | null => {
    const repo = members.active;
    if (repo !== null) {
      if (gitMissing(repo)) return null;
      const row = projects.view?.rows.find((r) => r.uid === repo.id) ?? null;
      const list = members.lists.get(repo.id) ?? null;
      return {
        prefix: t("projects.status.prefix", { short: row?.short ?? repo.path }),
        branch: repo.detached
          ? t("git.status.detached", { oid: cut(repo.oid) })
          : (repo.branch ?? ""),
        oid: repo.oid === null ? null : cut(repo.oid),
        detached: repo.detached,
        unborn: repo.unborn,
        changed:
          repo.changed === null
            ? ""
            : t("git.status.changed", { count: changedCount(repo.changed) }),
        state: STATE_KEYS[repo.state],
        problem: repo.error?.kind === "failed" ? t("projects.card.gitError") : null,
        baseline: snapshot?.baseline.kind ?? list?.baseline.kind ?? "lastVisit",
        backend: snapshot?.backend ?? list?.backend ?? null,
      };
    }
    if (snapshot === null) return null;
    const head = snapshot.head;
    const oid = head.oid === null ? null : cut(head.oid);
    return {
      prefix: null,
      branch: head.detached ? t("git.status.detached", { oid: oid ?? "" }) : (head.branch ?? ""),
      oid,
      detached: head.detached,
      unborn: head.unborn,
      changed: snapshot.truncated
        ? t("git.status.changedMore", { count: snapshot.files.length })
        : t("git.status.changed", { count: snapshot.files.length }),
      state: STATE_KEYS[snapshot.state],
      problem: null,
      baseline: snapshot.baseline.kind,
      backend: snapshot.backend,
    };
  });

  const BASELINES: { kind: BaselineKind; label: "git.baseline.visit" | "git.baseline.commit" }[] = [
    { kind: "lastVisit", label: "git.baseline.visit" },
    { kind: "head", label: "git.baseline.commit" },
  ];

  const LETTER_KEYS: Readonly<Record<Letter, MessageKey>> = {
    modified: "git.explain.modified",
    added: "git.explain.added",
    untracked: "git.explain.untracked",
    deleted: "git.explain.deleted",
    renamed: "git.explain.renamed",
    conflict: "git.explain.conflict",
  };

  let box: HTMLElement | undefined = $state();

  /** The status bar clips what overflows it, so the legend is fixed and put above the item when it opens. */
  function above(node: HTMLElement): void {
    if (!box) return;
    const anchor = box.getBoundingClientRect();
    const left = Math.min(anchor.left, window.innerWidth - node.offsetWidth - 8);
    node.style.left = `${String(Math.max(8, left))}px`;
    node.style.bottom = `${String(window.innerHeight - anchor.top + 6)}px`;
  }

  // closes on Escape | a click outside
  $effect(() => {
    if (!git.legend) return;
    const onKey = (e: KeyboardEvent): void => {
      if (e.key === "Escape") git.legend = false;
    };
    const onPointer = (e: PointerEvent): void => {
      if (box && e.target instanceof Node && !box.contains(e.target)) git.legend = false;
    };
    window.addEventListener("keydown", onKey);
    window.addEventListener("pointerdown", onPointer);
    return () => {
      window.removeEventListener("keydown", onKey);
      window.removeEventListener("pointerdown", onPointer);
    };
  });
</script>

{#if shown !== null}
  <span class="git" bind:this={box}>
    <button
      type="button"
      class="chip"
      aria-haspopup="dialog"
      aria-expanded={git.legend}
      title={`${shown.prefix === null ? "" : `${shown.prefix} `}${t("git.status.hint", {
        branch: shown.branch,
        oid: shown.oid ?? t("git.status.noCommits"),
      })}`}
      onclick={() => (git.legend = !git.legend)}
    >
      <GitBranch size={13} aria-hidden="true" />
      {#if shown.prefix !== null}<span class="prefix">{shown.prefix}</span>{/if}
      <span class="branch">{shown.branch}</span>
      {#if shown.unborn}
        <span class="muted">{t("git.status.noCommits")}</span>
      {:else if shown.oid && !shown.detached}
        <span class="muted oid">{shown.oid}</span>
      {/if}
      {#if shown.problem !== null}
        <span class="muted">{shown.problem}</span>
      {:else}
        <span class="muted" title={t("git.status.changedHint")}>{shown.changed}</span>
      {/if}
    </button>
    {#if shown.state}
      <span class="state" role="status">{t(shown.state)}</span>
    {/if}
    {#if git.legend}
      <div class="legend" role="dialog" aria-label={t("git.legend.title")} use:above>
        <p class="title">{t("git.legend.title")}</p>
        <ul>
          {#each LETTER_ORDER as letter (letter)}
            <li>
              <span class="mark tone-{letter}">{letterOf(letter)}</span>
              <span
                >{t(LETTER_KEYS[letter], {
                  since: t(shown.baseline === "lastVisit" ? "git.since.visit" : "git.since.commit"),
                })}</span
              >
            </li>
          {/each}
          <li><span class="mark tone-modified">•</span><span>{t("git.legend.folder")}</span></li>
        </ul>
        <div class="baseline" role="radiogroup" aria-label={t("git.baseline.label")}>
          <span class="label">{t("git.baseline.label")}</span>
          {#each BASELINES as option (option.kind)}
            <button
              type="button"
              role="radio"
              class={["segment", shown.baseline === option.kind && "is-on"]}
              aria-checked={shown.baseline === option.kind}
              onclick={() => void git.setBaseline(option.kind)}>{t(option.label)}</button
            >
          {/each}
        </div>
        <p class="note">
          {#if shown.backend !== null}
            {t(shown.backend === "system" ? "git.backend.usingSystem" : "git.backend.usingBuiltin")}
          {/if}
          {t("git.legend.readOnly")}
        </p>
      </div>
    {/if}
  </span>
{/if}

<style>
  .git {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: var(--sp-2);
  }
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 0 6px;
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    color: inherit;
    font: inherit;
    cursor: pointer;
  }
  .chip:hover,
  .chip[aria-expanded="true"] {
    background: var(--hover);
    color: var(--text);
  }
  .prefix {
    max-width: 14ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .branch {
    max-width: 18ch;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .muted {
    color: var(--muted);
  }
  .oid {
    font-family: var(--font-code);
  }
  .state {
    padding: 0 6px;
    border-radius: 999px;
    background: color-mix(in srgb, var(--git-modified) 18%, transparent);
    color: var(--git-modified-text);
    font-size: var(--fs-ui-xs);
    font-weight: 600;
    letter-spacing: 0.02em;
  }
  .legend {
    position: fixed;
    z-index: 40;
    width: 360px;
    max-width: calc(100vw - 24px);
    padding: var(--sp-3);
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
  ul {
    display: grid;
    gap: 6px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  li {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
  }
  .mark {
    flex: none;
    min-width: 18px;
    border-radius: 999px;
    text-align: center;
    font-weight: 700;
  }
  .tone-modified {
    color: var(--git-modified-text);
  }
  .tone-added,
  .tone-untracked {
    color: var(--git-added-text);
  }
  .tone-deleted,
  .tone-conflict {
    color: var(--git-deleted-text);
  }
  .tone-renamed {
    color: var(--git-renamed-text);
  }
  .baseline {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 4px;
    margin-top: var(--sp-3);
  }
  .label {
    margin-right: var(--sp-2);
    color: var(--muted);
  }
  .segment {
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .segment.is-on {
    border-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
    font-weight: 600;
  }
  .note {
    margin: var(--sp-3) 0 0;
    color: var(--muted);
  }
</style>
