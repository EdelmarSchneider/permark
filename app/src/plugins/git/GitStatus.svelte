<script lang="ts">
  // The git item of the status bar: branch, where HEAD is, how many files changed, and what the repository is doing
  // (merging, rebasing, ...). A click opens the legend: the letters in plain words, and what changes are counted from.
  import GitBranch from "@lucide/svelte/icons/git-branch";
  import type { BaselineKind, Letter } from "../../api";
  import { t, type MessageKey } from "../../core/i18n";
  import { LETTER_ORDER, git, letterOf } from "./store.svelte";

  const snapshot = $derived(git.snapshot);
  const shortOid = $derived(snapshot?.head.oid?.slice(0, 7) ?? null);
  const label = $derived.by(() => {
    const head = snapshot?.head;
    if (!head) return "";
    if (head.detached) return t("git.status.detached", { oid: shortOid ?? "" });
    return head.branch ?? "";
  });
  const changed = $derived(
    snapshot === null
      ? ""
      : snapshot.truncated
        ? t("git.status.changedMore", { count: snapshot.files.length })
        : t("git.status.changed", { count: snapshot.files.length }),
  );

  const STATES = {
    clean: null,
    merging: "git.state.merging",
    rebasing: "git.state.rebasing",
    cherryPicking: "git.state.cherryPicking",
    reverting: "git.state.reverting",
    bisecting: "git.state.bisecting",
    busy: "git.state.busy",
  } as const;
  const repoState = $derived(snapshot === null ? null : STATES[snapshot.state]);

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

{#if snapshot !== null}
  <span class="git" bind:this={box}>
    <button
      type="button"
      class="chip"
      aria-haspopup="dialog"
      aria-expanded={git.legend}
      title={t("git.status.hint", { branch: label, oid: shortOid ?? t("git.status.noCommits") })}
      onclick={() => (git.legend = !git.legend)}
    >
      <GitBranch size={13} aria-hidden="true" />
      <span class="branch">{label}</span>
      {#if snapshot.head.unborn}
        <span class="muted">{t("git.status.noCommits")}</span>
      {:else if shortOid && !snapshot.head.detached}
        <span class="muted oid">{shortOid}</span>
      {/if}
      <span class="muted" title={t("git.status.changedHint")}>{changed}</span>
    </button>
    {#if repoState}
      <span class="state" role="status">{t(repoState)}</span>
    {/if}
    {#if git.legend}
      <div class="legend" role="dialog" aria-label={t("git.legend.title")}>
        <p class="title">{t("git.legend.title")}</p>
        <ul>
          {#each LETTER_ORDER as letter (letter)}
            <li>
              <span class="mark tone-{letter}">{letterOf(letter)}</span>
              <span
                >{t(LETTER_KEYS[letter], {
                  since: t(
                    snapshot.baseline.kind === "lastVisit" ? "git.since.visit" : "git.since.commit",
                  ),
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
              class={["segment", snapshot.baseline.kind === option.kind && "is-on"]}
              aria-checked={snapshot.baseline.kind === option.kind}
              onclick={() => void git.setBaseline(option.kind)}>{t(option.label)}</button
            >
          {/each}
        </div>
        <p class="note">
          {t(
            snapshot.backend === "system" ? "git.backend.usingSystem" : "git.backend.usingBuiltin",
          )}
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
    position: absolute;
    bottom: calc(100% + 6px);
    left: 0;
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
