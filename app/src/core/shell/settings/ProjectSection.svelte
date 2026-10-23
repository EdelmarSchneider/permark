<script lang="ts">
  // This folder: what the project shares with its team in `.permark/config.toml` - which files the Explorer shows by
  // default, what is left out of the feed and of search, what agents never get. Each control is kept at once, in that
  // file only (the rest of the file stays as it is). Without `.permark/` the section only offers to create it.
  import type { TreeShow } from "../../../api";
  import { conditions } from "../../conditions/context.svelte";
  import { t, type MessageKey } from "../../i18n";
  import { contributions } from "../../plugins/instance.svelte";
  import { projects } from "../../projects/instance.svelte";
  import { askInitHere } from "../initProject";
  import Card from "./Card.svelte";
  import ListEditor from "./ListEditor.svelte";
  import { problemKey } from "./problems";
  import Segments from "./Segments.svelte";
  import SettingRow from "./SettingRow.svelte";
  import { projectSettings } from "./stores.svelte";

  const initialized = $derived(conditions.current.permark === "initialized");
  const loaded = $derived(projectSettings.state);
  const problem = $derived(projectSettings.problem);
  /** Parts plugins add (the agent's narrowing). */
  const extra = $derived(contributions.visible("projectSection", conditions.current));

  // read when the section shows, and again when PerMark gets initialized while it shows (external I/O)
  $effect(() => {
    void initialized;
    void projectSettings.load();
    return () => projectSettings.clearProblem();
  });

  /** The card fields load only where the folder holds projects (a plain folder never needs them). */
  const cardFields = (): Promise<typeof import("../../projects/CardFields.svelte")> =>
    import("../../projects/CardFields.svelte");

  type Preset = TreeShow | "unset";
  const PRESETS: { value: Preset; label: MessageKey }[] = [
    { value: "unset", label: "project.treeUnset" },
    { value: "documents", label: "filter.documents" },
    { value: "docs-and-assets", label: "filter.docsAndAssets" },
    { value: "everything", label: "filter.everything" },
    { value: "custom", label: "filter.custom" },
  ];
</script>

{#if projects.present && projects.view}
  {@const card = projects.view.rootCard}
  <!-- the opened folder's own card (it holds projects: its name shows where they are listed) -->
  <section class="group root-card" aria-labelledby="proj-card">
    <h3 id="proj-card">{t("projects.card.title")}</h3>
    <Card>
      <div class="card-body">
        {#await cardFields() then fields}
          <fields.default
            id="root-card"
            target={{ kind: "root" }}
            place={card.place}
            categoryStatus={projects.view.rootCategoryStatus}
            short={card.short}
            values={{
              name: card.name,
              short: card.short,
              description: card.description ?? "",
              category: card.category ?? "",
            }}
          />
        {/await}
      </div>
    </Card>
  </section>
{/if}
{#if !initialized}
  <Card>
    <SettingRow label={t("project.notInitialized")} hint={t("project.notInitializedHint")}>
      <button type="button" class="btn btn--primary btn--sm" onclick={() => askInitHere()}>
        {t("project.initButton")}
      </button>
    </SettingRow>
  </Card>
{:else if loaded.status === "ready"}
  {@const settings = loaded.settings}
  {#snippet problemNote(key: string)}
    {#if problem !== null && problem.key === key}
      <span class="problem" role="alert">{t(problemKey(problem.code))}</span>
    {/if}
  {/snippet}
  <p class="where">{t("project.where")} <code>.permark/config.toml</code></p>
  <section class="group" aria-labelledby="proj-tree">
    <h3 id="proj-tree">{t("project.tree")}</h3>
    <Card>
      <SettingRow
        label={t("project.treeShow")}
        labelId="proj-show"
        hint={t("project.treeShowHint")}
      >
        <Segments
          labelledby="proj-show"
          value={settings.tree.show ?? "unset"}
          options={PRESETS.map((o) => ({ value: o.value, label: t(o.label) }))}
          onchange={(value) =>
            void projectSettings.set({ key: "treeShow", value: value === "unset" ? null : value })}
        />
        {#snippet note()}{@render problemNote("treeShow")}{/snippet}
      </SettingRow>
      <SettingRow label={t("project.treeInclude")} hint={t("project.treeIncludeHint")} wide>
        <ListEditor
          items={settings.tree.include}
          label={t("project.treeInclude")}
          placeholder={t("project.patternPlaceholder")}
          onchange={(items) => void projectSettings.set({ key: "treeInclude", value: items })}
        />
        {#snippet note()}{@render problemNote("treeInclude")}{/snippet}
      </SettingRow>
      <SettingRow label={t("project.treeExclude")} hint={t("project.treeExcludeHint")} wide>
        <ListEditor
          items={settings.tree.exclude}
          label={t("project.treeExclude")}
          placeholder={t("project.patternPlaceholder")}
          onchange={(items) => void projectSettings.set({ key: "treeExclude", value: items })}
        />
        {#snippet note()}{@render problemNote("treeExclude")}{/snippet}
      </SettingRow>
    </Card>
  </section>
  <section class="group" aria-labelledby="proj-ignore">
    <h3 id="proj-ignore">{t("project.ignore")}</h3>
    <Card>
      <SettingRow label={t("project.ignoreLabel")} hint={t("project.ignoreHint")} wide>
        <ListEditor
          items={settings.ignore}
          label={t("project.ignoreLabel")}
          placeholder={t("project.patternPlaceholder")}
          onchange={(items) => void projectSettings.set({ key: "ignore", value: items })}
        />
        {#snippet note()}{@render problemNote("ignore")}{/snippet}
      </SettingRow>
    </Card>
  </section>
  {#each extra as section (section.owner + section.id)}
    <section class="group" aria-labelledby={`proj-${section.owner}-${section.id}`}>
      <h3 id={`proj-${section.owner}-${section.id}`}>{t(section.label)}</h3>
      <Card>
        {#await section.view() then loaded}
          {@const Section = loaded.default}
          <Section />
        {/await}
      </Card>
    </section>
  {/each}
{:else if loaded.status === "failed"}
  <p class="quiet" role="alert">{t("project.loadFailed")}</p>
{:else}
  <p class="quiet">{t("app.loading")}</p>
{/if}

<style>
  .where,
  .quiet {
    margin: 0 0 var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  code {
    font-family: var(--font-code);
    font-size: var(--fs-ui-xs);
  }
  .group + .group {
    margin-top: var(--sp-5);
  }
  .root-card {
    margin-bottom: var(--sp-5);
  }
  .card-body {
    padding: var(--sp-3) var(--sp-4);
  }
  h3 {
    margin: 0 0 var(--sp-2);
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .problem {
    color: var(--git-deleted-text);
  }
</style>
