<script lang="ts">
  // Settings > Projects inside: the opened folder's official tags and categories (`projects.md` §7.1), each with its
  // look, and a field per kind to add a name (written at once to `.permark/vocabulary.toml`, said below the lists).
  // Minimal on purpose: renaming, removing and changing a look are done in the file ("Show the file"); they need
  // backend ops of their own (deferred).
  import { tick } from "svelte";
  import type { VocabularyKind } from "../../api";
  import { documents } from "../doc/document.svelte";
  import { t } from "../i18n";
  import { ui } from "../shell/ui.svelte";
  import Card from "../shell/settings/Card.svelte";
  import { projects } from "./instance.svelte";
  import { officialOf, promoteBlocked, vocabularyName, vocabularyPlace } from "./vocabulary";

  const FILE = ".permark/vocabulary.toml";

  const view = $derived(projects.view);
  const root = $derived(projects.rootName);
  const file = $derived(view?.vocabulary.file ?? { status: "absent" as const });
  const blocked = $derived(promoteBlocked(view));
  /** The file can't be read | a newer PerMark wrote it: said once, on top. */
  const problem = $derived(
    file.status === "unreadable"
      ? t("projects.vocabulary.unreadable", { line: file.line })
      : file.status === "newer"
        ? t("projects.vocabulary.newer")
        : null,
  );

  let typed = $state<Record<VocabularyKind, string>>({ tag: "", category: "" });
  let said = $state<Record<VocabularyKind, string>>({ tag: "", category: "" });
  let busy = $state(false);
  let fields: Record<VocabularyKind, HTMLInputElement | undefined> = $state({
    tag: undefined,
    category: undefined,
  });

  const KINDS: readonly {
    kind: VocabularyKind;
    title: "projects.vocabulary.tags" | "projects.vocabulary.categories";
    add: "projects.vocabulary.addTag" | "projects.vocabulary.addCategory";
  }[] = [
    { kind: "tag", title: "projects.vocabulary.tags", add: "projects.vocabulary.addTag" },
    {
      kind: "category",
      title: "projects.vocabulary.categories",
      add: "projects.vocabulary.addCategory",
    },
  ];

  async function add(kind: VocabularyKind): Promise<void> {
    const name = vocabularyName(typed[kind]);
    if (name === null || busy || blocked !== null) return;
    if (officialOf(view, kind).some((e) => e.name === name)) {
      said[kind] = t("projects.vocabulary.already", { name });
      return;
    }
    busy = true;
    const done = await projects.promote(kind, name);
    busy = false;
    if (done) {
      typed[kind] = "";
      said[kind] = "";
    }
    await tick();
    fields[kind]?.focus();
  }

  function showFile(): void {
    ui.closeSettingsPage();
    documents.navigate(FILE, null);
  }
</script>

<section class="vocabulary" aria-labelledby="vocab-title" data-vocabulary>
  <h4 class="group" id="vocab-title">{t("projects.vocabulary.title")}</h4>
  <Card>
    <div class="body">
      <p class="intro">{t("projects.vocabulary.intro", { root })}</p>
      {#if problem !== null}<p class="warn" role="alert">{problem}</p>{/if}
      {#each KINDS as k (k.kind)}
        {@const list = officialOf(view, k.kind)}
        {@const empty = vocabularyName(typed[k.kind]) === null}
        <div class="kind">
          <span class="kind-title" id="vocab-{k.kind}">{t(k.title)}</span>
          {#if list.length === 0}
            <p class="quiet">{t("projects.vocabulary.none")}</p>
          {:else}
            <ul class="names" aria-labelledby="vocab-{k.kind}">
              {#each list as entry (entry.name)}
                <li
                  class="name"
                  class:tinted={entry.color !== null}
                  style:--c={entry.color === null ? null : `var(--tag-${entry.color})`}
                  title={entry.name}
                >
                  {entry.name}
                </li>
              {/each}
            </ul>
          {/if}
          <form
            class="add"
            onsubmit={(e) => {
              e.preventDefault();
              void add(k.kind);
            }}
          >
            <input
              type="text"
              class="field-input"
              maxlength={40}
              autocomplete="off"
              spellcheck="false"
              aria-label={t(k.add)}
              placeholder={t(k.add)}
              aria-describedby="vocab-where vocab-said-{k.kind}"
              disabled={blocked !== null}
              bind:value={typed[k.kind]}
              bind:this={fields[k.kind]}
              oninput={() => (said[k.kind] = "")}
            />
            <button
              type="submit"
              class="btn btn--sm"
              disabled={busy || empty || blocked !== null}
              title={blocked ?? (empty ? t("projects.vocabulary.typeName") : undefined)}
              >{t("projects.vocabulary.add")}</button
            >
          </form>
          <p class="quiet said" id="vocab-said-{k.kind}" role="status">{said[k.kind]}</p>
        </div>
      {/each}
      <p class="quiet" id="vocab-where">{t(vocabularyPlace(view))}</p>
      {#if file.status !== "absent"}
        <p class="quiet hint">
          {t("projects.vocabulary.editHint")}
          <button type="button" class="btn btn--sm" onclick={showFile}
            >{t("projects.record.show")}</button
          >
        </p>
      {/if}
    </div>
  </Card>
</section>

<style>
  .group {
    margin: var(--sp-4) 0 var(--sp-2);
    padding-left: var(--sp-1);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    font-weight: 600;
  }
  .body {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    padding: var(--sp-3) var(--sp-4);
    font-size: var(--fs-ui-sm);
  }
  p {
    margin: 0;
  }
  .intro,
  .quiet {
    color: var(--muted);
  }
  .warn {
    color: var(--git-deleted-text);
  }
  .kind {
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    min-width: 0;
  }
  .kind-title {
    font-weight: 600;
  }
  .names {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .name {
    max-width: 100%;
    overflow: hidden;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface-2);
    font-size: var(--fs-ui-xs);
    line-height: 18px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .name.tinted {
    border-color: color-mix(in srgb, var(--c) 28%, transparent);
    background: color-mix(in srgb, var(--c) 12%, transparent);
    color: var(--c);
  }
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    align-items: center;
  }
  .add input {
    flex: 0 1 240px;
    min-width: 0;
  }
  .hint {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
  }
</style>
