<script lang="ts">
  // Settings > Projects: the projects inside the opened folder in three groups (Included | Not included | Not found),
  // a filter with bulk actions on the rows it shows, each included project's rules and card. Consent mode (strip
  // "Choose…"): the offered projects, all checked; nothing is saved until "Include checked".
  import { tick } from "svelte";
  import type { ProjectRow } from "../../api";
  import { documents } from "../doc/document.svelte";
  import { t, tn, type MessageKey } from "../i18n";
  import { ui } from "../shell/ui.svelte";
  import Card from "../shell/settings/Card.svelte";
  import { projects } from "./instance.svelte";
  import { depthOf, groupsOf, matches } from "./model";
  import ProjectRowView from "./ProjectRowView.svelte";

  const RECORD = ".permark/projects.toml";

  let filter = $state("");
  let open = $state.raw<ReadonlySet<string>>(new Set());
  let section: HTMLElement | undefined = $state();

  const view = $derived(projects.view);
  const choosing = $derived(projects.choosing !== null);
  const rootName = $derived(view?.rootCard.name ?? "");
  const byKey = $derived(new Map((view?.rows ?? []).map((r) => [r.key, r])));
  const groups = $derived(groupsOf(view?.rows ?? [], filter));
  /** Consent mode lists the offered projects only. */
  const offered = $derived(
    (view?.rows ?? [])
      .filter((r) => r.state === "offered" && matches(r, filter))
      .sort((a, b) => (a.key < b.key ? -1 : 1)),
  );
  const shown = $derived(
    choosing ? offered : [...groups.included, ...groups.notIncluded, ...groups.notFound],
  );
  const canInclude = $derived(
    choosing
      ? offered.some((r) => !(projects.choosing?.has(r.path) ?? false))
      : groups.notIncluded.length > 0,
  );
  const canLeaveOut = $derived(
    choosing
      ? offered.some((r) => projects.choosing?.has(r.path) ?? false)
      : groups.included.length > 0,
  );
  const problems = $derived.by((): string[] => {
    if (!view) return [];
    const lines: string[] = [];
    if (view.record.status === "unreadable") {
      lines.push(t("projects.record.unreadable", { line: view.record.line }));
    } else if (view.record.status === "newer") {
      lines.push(t("projects.record.newer"));
    }
    const first = view.skipped[0];
    if (first) {
      lines.push(
        tn("projects.record.skipped", view.skipped.length, {
          line: first.line,
          reason: t(`projects.reason.${first.reason}` as MessageKey),
        }),
      );
    }
    return lines;
  });

  function toggle(row: ProjectRow): void {
    const next = new Set(open);
    if (!next.delete(row.key)) next.add(row.key);
    open = next;
  }

  function includeShown(): void {
    if (choosing) for (const r of offered) projects.setChosen(r.path, true);
    else void projects.includeShown(groups.notIncluded);
  }

  function leaveOutShown(): void {
    if (choosing) for (const r of offered) projects.setChosen(r.path, false);
    else void projects.leaveOutShown(groups.included);
  }

  function showRecord(): void {
    ui.closeSettingsPage();
    documents.navigate(RECORD, null);
  }

  /** Back in the Explorer after the page closed: focus on its list (the strip that had it is gone). */
  async function confirm(): Promise<void> {
    await projects.confirmChoose();
    await tick();
    document.querySelector<HTMLElement>('[role="tree"]')?.focus();
  }

  // "Project settings…" of a node: that row opens, comes into view, focus on its first rule
  $effect(() => {
    const ask = projects.focusRow;
    if (ask === null || !section) return;
    projects.focusRow = null;
    filter = "";
    open = new Set([...open, ask.key]);
    void tick().then(() => {
      const row = section?.querySelector<HTMLElement>(
        `[data-project="${CSS.escape(projects.rowByKey(ask.key)?.path ?? "")}"]`,
      );
      row?.scrollIntoView({ block: "nearest" });
      row?.querySelector<HTMLElement>('.rules-body [role="radio"][tabindex="0"]')?.focus();
    });
  });

  // leaving the page in consent mode (Close, Esc) saves nothing
  $effect(() => () => {
    if (projects.choosing !== null) projects.choosing = null;
  });
</script>

{#snippet rows(list: readonly ProjectRow[], mode: "normal" | "choose")}
  <Card>
    {#each list as row (row.key)}
      <ProjectRowView
        {row}
        {mode}
        {rootName}
        depth={depthOf(row, byKey)}
        expanded={open.has(row.key)}
        toggle={() => toggle(row)}
      />
    {/each}
  </Card>
{/snippet}

<div class="projects" bind:this={section}>
  {#if choosing}
    <h3 class="choose-title" tabindex="-1" data-autofocus>{t("projects.choose.title")}</h3>
  {/if}
  <p class="intro">{t("projects.settings.intro", { root: rootName })}</p>

  {#if problems.length > 0}
    <div class="error-card" role="alert">
      {#each problems as line (line)}<p>{line}</p>{/each}
      <button type="button" class="btn btn--sm" onclick={showRecord}>
        {t("projects.record.show")}
      </button>
    </div>
  {/if}

  {#if !view || !view.ready}
    <p class="quiet" role="status">{t("projects.settings.looking")}</p>
  {:else}
    <div class="tools">
      <input
        type="search"
        class="field-input filter"
        placeholder={t("projects.settings.filter")}
        aria-label={t("projects.settings.filter")}
        bind:value={filter}
      />
      <span class="spacer"></span>
      {#if canInclude}
        <button
          type="button"
          class="btn btn--sm"
          disabled={projects.readOnly && !choosing}
          title={projects.readOnly && !choosing ? t("projects.record.locked") : undefined}
          onclick={includeShown}>{t("projects.settings.includeShown")}</button
        >
      {/if}
      {#if canLeaveOut}
        <button
          type="button"
          class="btn btn--sm"
          disabled={projects.readOnly && !choosing}
          title={projects.readOnly && !choosing ? t("projects.record.locked") : undefined}
          onclick={leaveOutShown}>{t("projects.settings.leaveOutShown")}</button
        >
      {/if}
    </div>

    {#if shown.length === 0}
      <p class="quiet" role="status">{t("projects.settings.noMatch")}</p>
    {:else if choosing}
      {@render rows(offered, "choose")}
    {:else}
      {#if groups.included.length > 0}
        <h4 class="group">{t("projects.group.included", { n: groups.included.length })}</h4>
        {@render rows(groups.included, "normal")}
      {/if}
      {#if groups.notIncluded.length > 0}
        <h4 class="group">{t("projects.group.notIncluded", { n: groups.notIncluded.length })}</h4>
        {@render rows(groups.notIncluded, "normal")}
      {/if}
      {#if groups.notFound.length > 0}
        <h4 class="group">{t("projects.group.notFound", { n: groups.notFound.length })}</h4>
        {@render rows(groups.notFound, "normal")}
      {/if}
    {/if}

    {#if choosing}
      <div class="footer">
        <button type="button" class="btn btn--primary" onclick={() => void confirm()}>
          {t("projects.choose.confirm")}
        </button>
        <button type="button" class="btn" onclick={() => projects.cancelChoose()}>
          {t("common.cancel")}
        </button>
      </div>
    {:else}
      <p class="quiet saved">{t("projects.settings.savedIn")}</p>
    {/if}
  {/if}
</div>

<style>
  .choose-title {
    margin: 0 0 var(--sp-2);
    font-size: var(--fs-ui);
    font-weight: 600;
    outline: none;
  }
  .choose-title:focus-visible {
    box-shadow: 0 0 0 2px var(--accent-muted);
    border-radius: 2px;
  }
  .intro,
  .quiet {
    margin: 0 0 var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .saved {
    margin-top: var(--sp-4);
  }
  .error-card {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: var(--sp-2);
    margin: 0 0 var(--sp-4);
    padding: var(--sp-3) var(--sp-4);
    border: 1px solid var(--git-deleted-text);
    border-radius: var(--radius-card);
    background: var(--surface);
    font-size: var(--fs-ui-sm);
  }
  .error-card p {
    margin: 0;
  }
  .tools {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
    margin: 0 0 var(--sp-3);
  }
  .filter {
    flex: 0 1 260px;
    min-width: 0;
  }
  .spacer {
    flex: 1;
  }
  .group {
    margin: var(--sp-4) 0 var(--sp-2);
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .footer {
    position: sticky;
    bottom: 0;
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    margin-top: var(--sp-4);
    padding: var(--sp-3) 0;
    background: var(--bg);
  }
</style>
