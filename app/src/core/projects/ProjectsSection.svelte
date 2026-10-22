<script lang="ts">
  // Settings > Projects inside: the projects inside the opened folder in three groups (Included | Not included | Not
  // found), a filter with bulk actions on the rows it shows, each included project's details and rules. Consent mode
  // (strip "Choose…"): the offered projects, all checked; nothing is saved until "Include checked". A row that moves
  // to another group (or goes) hands the focus to the next row of its group.
  import { tick, untrack } from "svelte";
  import type { ProjectRow } from "../../api";
  import { conditions } from "../conditions/context.svelte";
  import { documents } from "../doc/document.svelte";
  import { t, tn, type MessageKey } from "../i18n";
  import { contributions } from "../plugins/instance.svelte";
  import { ui } from "../shell/ui.svelte";
  import Card from "../shell/settings/Card.svelte";
  import { projects } from "./instance.svelte";
  import { depthOf, groupsOf, matches, recordPlace } from "./model";
  import ProjectRowView from "./ProjectRowView.svelte";
  import VocabularySection from "./VocabularySection.svelte";

  const RECORD = ".permark/projects.toml";

  let filter = $state("");
  let open = $state.raw<ReadonlySet<string>>(new Set());
  let section: HTMLElement | undefined = $state();
  let filterInput: HTMLInputElement | undefined = $state();
  let problem: HTMLElement | undefined = $state();

  const view = $derived(projects.view);
  /** One sentence plugins add to the intro (git missing). */
  const notes = $derived(
    contributions.visible("projectFacts", conditions.current).flatMap((f) => f.note() ?? []),
  );

  // the included rows in view: plugins fetch their facts for those (git: changed files)
  let inView = $state.raw<ReadonlySet<string>>(new Set());
  const watched = new Map<Element, string>();
  let observer: IntersectionObserver | null = null;

  /** Called from the rows' effects: reading the set must not make them depend on it. */
  function see(uid: string, on: boolean): void {
    untrack(() => {
      if (inView.has(uid) === on) return;
      const next = new Set(inView);
      if (on) next.add(uid);
      else next.delete(uid);
      inView = next;
    });
  }

  /** A row's element followed while it exists (no observer, as in tests: always in view). */
  function watch(el: HTMLElement, uid: string): () => void {
    watched.set(el, uid);
    if (observer === null) see(uid, true);
    else observer.observe(el);
    return () => {
      watched.delete(el);
      observer?.unobserve(el);
      if (![...watched.values()].includes(uid)) see(uid, false);
    };
  }

  $effect(() => {
    if (typeof IntersectionObserver === "undefined") return;
    const seen = new IntersectionObserver((entries) => {
      for (const entry of entries) {
        const uid = watched.get(entry.target);
        if (uid !== undefined) see(uid, entry.isIntersecting);
      }
    });
    observer = seen;
    for (const el of watched.keys()) seen.observe(el);
    return () => {
      seen.disconnect();
      if (observer === seen) observer = null;
    };
  });
  $effect(() => {
    projects.look("settings", "glance", [...inView]);
  });
  $effect(() => () => projects.look("settings", "glance", []));
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
  /** Why a bulk button is off (it stays in place, S2-9): the list can't be changed | nothing listed | nothing to do. */
  const includeWhy = $derived(
    projects.readOnly && !choosing
      ? t("projects.record.locked")
      : shown.length === 0
        ? t("projects.settings.noMatch")
        : canInclude
          ? undefined
          : t(choosing ? "projects.choose.allChecked" : "projects.settings.allIncluded"),
  );
  const leaveOutWhy = $derived(
    projects.readOnly && !choosing
      ? t("projects.record.locked")
      : shown.length === 0
        ? t("projects.settings.noMatch")
        : canLeaveOut
          ? undefined
          : t(choosing ? "projects.choose.noneChecked" : "projects.settings.noneIncluded"),
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

  /** The row the focus goes to when `row` leaves its group: the next one, else the one before (`null`: none left). */
  function neighbor(row: ProjectRow): ProjectRow | null {
    const group =
      row.state === "included"
        ? groups.included
        : row.state === "missing"
          ? groups.notFound
          : groups.notIncluded;
    const at = group.findIndex((r) => r.key === row.key);
    if (at < 0) return null;
    return group[at + 1] ?? group[at - 1] ?? null;
  }

  function rowElement(path: string): HTMLElement | null {
    return section?.querySelector<HTMLElement>(`[data-project="${CSS.escape(path)}"]`) ?? null;
  }

  /** Focus on a row's control: its checkbox (`box`) | its first action button (missing rows) | its first rule. */
  function focusRow(path: string, what: "box" | "action" | "rule"): boolean {
    const row = rowElement(path);
    if (!row) return false;
    const selector =
      what === "box"
        ? 'input[type="checkbox"]'
        : what === "rule"
          ? '.rules-body [role="radio"][tabindex="0"]'
          : ".actions button:not([disabled])";
    const target =
      row.querySelector<HTMLElement>(selector) ??
      row.querySelector<HTMLElement>("button:not([disabled]), input:not([disabled])");
    if (!target) return false;
    row.scrollIntoView({ block: "nearest" });
    target.focus();
    return true;
  }

  /** The checkbox of a row: it moves to the other group; focus to the next row of the group it left (none left:
   *  the moved row, in its new group). */
  async function include(row: ProjectRow, on: boolean): Promise<void> {
    const next = neighbor(row);
    const moved = await projects.setIncluded(row, on);
    if (!moved) return;
    await tick();
    if (next === null || !focusRow(next.path, "box")) focusRow(row.path, "box");
  }

  /** "Remove from list": the row goes (Undo in the notice); focus to the next missing row, else the filter. */
  async function remove(row: ProjectRow): Promise<void> {
    const next = neighbor(row);
    projects.remove(row);
    await tick();
    if (next === null || !focusRow(next.path, "action")) filterInput?.focus();
  }

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

  // what the page was opened for: "Project settings…" of a node | card, a Changes row, an undone removal (that row
  // opens, comes into view, focus on its first rule | action), the broken list strip (focus on the problem); consent
  // mode: the heading. After the page's own focus (it runs first).
  $effect(() => {
    const ask = projects.ask;
    if (!section || !view || ask === null) return;
    projects.ask = null;
    if (ask.kind === "problem") {
      void tick().then(() => problem?.focus());
      return;
    }
    const row = projects.rowByKey(ask.key);
    if (row === null) return;
    filter = "";
    if (row.state === "included") open = new Set([...open, ask.key]);
    void tick().then(() => {
      focusRow(
        row.path,
        row.state === "included" ? "rule" : row.state === "missing" ? "action" : "box",
      );
    });
  });

  // consent mode: focus on its heading once the section shows (it loads after the page took the focus)
  $effect(() => {
    if (untrack(() => projects.choosing === null)) return;
    void tick().then(() => section?.querySelector<HTMLElement>("[data-autofocus]")?.focus());
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
        include={(on) => include(row, on)}
        remove={() => remove(row)}
        {watch}
      />
    {/each}
  </Card>
{/snippet}

<div class="projects" bind:this={section}>
  {#if choosing}
    <h3 class="choose-title" tabindex="-1" data-autofocus>{t("projects.choose.title")}</h3>
  {/if}
  <p class="intro">{t("projects.settings.intro", { root: rootName })}</p>
  {#each notes as note (note)}<p class="quiet" data-projects-note>{note}</p>{/each}

  {#if problems.length > 0}
    <div class="error-card" role="alert" tabindex="-1" bind:this={problem}>
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
        bind:this={filterInput}
      />
      <span class="spacer"></span>
      <button
        type="button"
        class="btn btn--sm"
        disabled={includeWhy !== undefined}
        title={includeWhy}
        onclick={includeShown}>{t("projects.settings.includeShown")}</button
      >
      <button
        type="button"
        class="btn btn--sm"
        disabled={leaveOutWhy !== undefined}
        title={leaveOutWhy}
        onclick={leaveOutShown}>{t("projects.settings.leaveOutShown")}</button
      >
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
      <!-- before the answer: where it is saved (a file in this folder) -->
      <p class="quiet saved">{t(recordPlace(view))}</p>
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
      <!-- the official lists live in this folder's .permark/ (there once a project is included) -->
      {#if view.record.status !== "absent" || view.vocabulary.file.status !== "absent"}
        <VocabularySection />
      {/if}
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
  .error-card:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
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
