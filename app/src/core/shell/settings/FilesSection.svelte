<script lang="ts">
  // This folder's files: what it shares with everyone in `.permark/config.toml` - which files the Explorer shows by
  // default, what is left out of search and of the live feed. Each control is kept at once, in that file only (the
  // rest of the file stays as it is). Without `.permark/` the rows show what they would hold, greyed, beside the offer
  // to create it: nothing is written into the folder until the person answers the Initialize question. A shared file
  // that does not read greys them too (the backend refuses every change to it until it is fixed by hand).
  import type { TreeShow } from "../../../api";
  import { t, type MessageKey } from "../../i18n";
  import Card from "./Card.svelte";
  import ListEditor from "./ListEditor.svelte";
  import { problemKey } from "./problems";
  import Segments from "./Segments.svelte";
  import SettingRow from "./SettingRow.svelte";
  import ShareInit from "./ShareInit.svelte";
  import { projectSettings } from "./stores.svelte";

  const loaded = $derived(projectSettings.state);
  const problem = $derived(projectSettings.problem);

  type Preset = TreeShow | "unset";
  const PRESETS: { value: Preset; label: MessageKey }[] = [
    { value: "unset", label: "project.treeUnset" },
    { value: "documents", label: "filter.documents" },
    { value: "docs-and-assets", label: "filter.docsAndAssets" },
    { value: "everything", label: "filter.everything" },
    { value: "custom", label: "filter.custom" },
  ];
</script>

{#if loaded.status === "ready"}
  {@const settings = loaded.settings}
  {@const off = projectSettings.sharedLocked}
  {#snippet problemNote(key: string)}
    {#if problem !== null && problem.key === key}
      <span class="problem" role="alert">{t(problemKey(problem.code))}</span>
    {/if}
  {/snippet}
  {#if off}
    <ShareInit compact />
  {/if}
  <div class="shared" class:off>
    <section class="group" aria-labelledby="proj-tree">
      <h4 id="proj-tree">{t("project.tree")}</h4>
      <Card>
        <SettingRow
          label={t("project.treeShow")}
          labelId="proj-show"
          hint={t("project.treeShowHint")}
          scope="everyone"
        >
          <Segments
            labelledby="proj-show"
            value={settings.tree.show ?? "unset"}
            options={PRESETS.map((o) => ({ value: o.value, label: t(o.label) }))}
            disabled={off}
            onchange={(value) =>
              void projectSettings.set({
                key: "treeShow",
                value: value === "unset" ? null : value,
              })}
          />
          {#snippet note()}{@render problemNote("treeShow")}{/snippet}
        </SettingRow>
        <SettingRow
          label={t("project.treeInclude")}
          hint={t("project.treeIncludeHint")}
          scope="everyone"
          wide
        >
          <ListEditor
            items={settings.tree.include}
            label={t("project.treeInclude")}
            placeholder={t("project.patternPlaceholder")}
            disabled={off}
            onchange={(items) => void projectSettings.set({ key: "treeInclude", value: items })}
          />
          {#snippet note()}{@render problemNote("treeInclude")}{/snippet}
        </SettingRow>
        <SettingRow
          label={t("project.treeExclude")}
          hint={t("project.treeExcludeHint")}
          scope="everyone"
          wide
        >
          <ListEditor
            items={settings.tree.exclude}
            label={t("project.treeExclude")}
            placeholder={t("project.patternPlaceholder")}
            disabled={off}
            onchange={(items) => void projectSettings.set({ key: "treeExclude", value: items })}
          />
          {#snippet note()}{@render problemNote("treeExclude")}{/snippet}
        </SettingRow>
      </Card>
    </section>
    <section class="group" aria-labelledby="proj-ignore">
      <h4 id="proj-ignore">{t("project.ignore")}</h4>
      <Card>
        <SettingRow
          label={t("project.ignoreLabel")}
          hint={t("project.ignoreHint")}
          scope="everyone"
          wide
        >
          <ListEditor
            items={settings.ignore}
            label={t("project.ignoreLabel")}
            placeholder={t("project.patternPlaceholder")}
            disabled={off}
            onchange={(items) => void projectSettings.set({ key: "ignore", value: items })}
          />
          {#snippet note()}{@render problemNote("ignore")}{/snippet}
        </SettingRow>
      </Card>
    </section>
  </div>
{:else if loaded.status === "failed"}
  <p class="quiet" role="alert">{t("project.loadFailed")}</p>
{:else}
  <p class="quiet">{t("app.loading")}</p>
{/if}

<style>
  .quiet {
    margin: 0 0 var(--sp-4);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  /* without .permark/ (or with a shared file that does not read) the shared rows only show what they hold
     (their fields grey; their buttons grey by their own disabled look): labels and hints keep their contrast */
  .shared.off :global(input),
  .shared.off :global(select) {
    opacity: 0.6;
  }
  .group + .group {
    margin-top: var(--sp-5);
  }
  h4 {
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
