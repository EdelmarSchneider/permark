<script lang="ts">
  // Settings > Projects inside: the opened folder's official tags and categories (`projects.md` §7.1). Per name:
  // Rename (in place), Color and icon (pickers in place, applied as picked), Remove (asks first, in place). A field per
  // kind adds a name (written at once; "Added …" with Undo). Renaming | removing changes only this list: files and
  // project cards keep the names they use (then unlisted), said where it happens. "Show the file" stays for hand edits.
  import { tick, untrack } from "svelte";
  import type { VocabularyEntry, VocabularyKind } from "../../api";
  import { documents } from "../doc/document.svelte";
  import { t } from "../i18n";
  import { TAG_ICONS } from "../looks/icons";
  import LookPicker from "../looks/LookPicker.svelte";
  import { isColor, isIcon } from "../looks/model";
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

  /** What one name is doing now (one at a time). */
  type Editing =
    | { mode: "none" }
    | { mode: "rename"; of: VocabularyKind; name: string }
    | { mode: "remove"; of: VocabularyKind; name: string }
    | { mode: "look"; of: VocabularyKind; name: string };
  let editing = $state<Editing>({ mode: "none" });
  /** The last name added per kind (Undo takes it off again); cleared by typing | another action. */
  let added = $state<Record<VocabularyKind, string | null>>({ tag: null, category: null });

  let typed = $state<Record<VocabularyKind, string>>({ tag: "", category: "" });
  let said = $state<Record<VocabularyKind, string>>({ tag: "", category: "" });
  let busy = $state(false);
  let section: HTMLElement | undefined = $state();
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

  function is(mode: Editing["mode"], of: VocabularyKind, name: string): boolean {
    return (
      editing.mode !== "none" && editing.mode === mode && editing.of === of && editing.name === name
    );
  }

  /** An element of a name's row (`data-official` = kind + name; `act` = one of its buttons | fields). */
  function rowPart(of: VocabularyKind, name: string, act: string): HTMLElement | null {
    if (!section) return null;
    for (const li of section.querySelectorAll<HTMLElement>("[data-official]")) {
      if (li.dataset["official"] === `${of}:${name}`) {
        return li.querySelector<HTMLElement>(`[data-act="${act}"]`);
      }
    }
    return null;
  }

  async function focusPart(of: VocabularyKind, name: string, act: string): Promise<void> {
    await tick();
    rowPart(of, name, act)?.focus();
  }

  async function add(kind: VocabularyKind): Promise<void> {
    const name = vocabularyName(typed[kind]);
    if (name === null || busy || blocked !== null) return;
    if (officialOf(view, kind).some((e) => e.name === name)) {
      said[kind] = t("projects.vocabulary.already", { name });
      added[kind] = null;
      return;
    }
    busy = true;
    const done = await projects.promote(kind, name);
    busy = false;
    if (done) {
      typed[kind] = "";
      said[kind] = t("projects.vocabulary.added", { name });
      added[kind] = name;
    }
    await tick();
    fields[kind]?.focus();
  }

  /** Undo of the last add: the name leaves the list again. */
  async function undoAdd(kind: VocabularyKind): Promise<void> {
    const name = added[kind];
    if (name === null || busy) return;
    busy = true;
    const done = await projects.removeOfficial(kind, name, true);
    busy = false;
    if (done) {
      added[kind] = null;
      said[kind] = t("projects.vocabulary.undone", { name });
    }
    await tick();
    fields[kind]?.focus();
  }

  function begin(next: Editing): void {
    editing = next;
    added = { tag: null, category: null };
  }

  async function startRename(of: VocabularyKind, name: string): Promise<void> {
    begin({ mode: "rename", of, name });
    await tick();
    const input = rowPart(of, name, "new-name");
    if (input instanceof HTMLInputElement) {
      input.focus();
      input.select();
    }
  }

  async function saveRename(
    of: VocabularyKind,
    name: string,
    input: HTMLInputElement,
  ): Promise<void> {
    const to = vocabularyName(input.value);
    if (busy) return;
    if (to === null || to === name) {
      await cancel(of, name, "rename");
      return;
    }
    busy = true;
    const done = await projects.renameOfficial(of, name, to);
    busy = false;
    if (done) {
      editing = { mode: "none" };
      await focusPart(of, to, "rename");
    } else {
      input.focus();
    }
  }

  async function startRemove(of: VocabularyKind, name: string): Promise<void> {
    begin({ mode: "remove", of, name });
    await focusPart(of, name, "confirm-remove");
  }

  async function confirmRemove(of: VocabularyKind, name: string): Promise<void> {
    if (busy) return;
    const list = officialOf(view, of);
    const at = list.findIndex((e) => e.name === name);
    // focus goes to the next name (else the one before; none left: the add field)
    const next = list[at + 1] ?? list[at - 1] ?? null;
    busy = true;
    const done = await projects.removeOfficial(of, name);
    busy = false;
    if (!done) {
      await focusPart(of, name, "confirm-remove");
      return;
    }
    editing = { mode: "none" };
    await tick();
    if (next !== null) rowPart(of, next.name, "rename")?.focus();
    else fields[of]?.focus();
  }

  async function toggleLook(of: VocabularyKind, name: string): Promise<void> {
    if (is("look", of, name)) {
      editing = { mode: "none" };
      return;
    }
    begin({ mode: "look", of, name });
    await tick();
    rowPart(of, name, "look-panel")?.querySelector<HTMLElement>("button")?.focus();
  }

  /** Back to the name's own button (Cancel, Esc, Done). */
  async function cancel(of: VocabularyKind, name: string, act: string): Promise<void> {
    editing = { mode: "none" };
    await focusPart(of, name, act);
  }

  function escape(e: KeyboardEvent, of: VocabularyKind, name: string, act: string): void {
    if (e.key !== "Escape" || e.isComposing) return;
    // Esc closes what is open here, not the settings page
    e.preventDefault();
    e.stopPropagation();
    void cancel(of, name, act);
  }

  function renameKey(e: KeyboardEvent, of: VocabularyKind, name: string): void {
    if (e.isComposing || e.keyCode === 229) return;
    if (e.key === "Enter") {
      e.preventDefault();
      void saveRename(of, name, e.currentTarget as HTMLInputElement);
    } else escape(e, of, name, "rename");
  }

  function pickLook(
    of: VocabularyKind,
    entry: VocabularyEntry,
    color: string | null,
    icon: string | null,
  ): void {
    void projects.setOfficialStyle(of, entry.name, color, icon);
  }

  function showFile(): void {
    ui.closeSettingsPage();
    documents.navigate(FILE, null);
  }

  // "Edit the official list…" (a tag's look | the Tags tab): that name's look opens, focus on its first choice; no name
  // (or one no longer listed): the list's add field
  $effect(() => {
    const ask = projects.ask;
    if (!section || !view || ask === null || ask.kind !== "vocabulary") return;
    projects.ask = null;
    untrack(() => {
      const listed = ask.name !== null && officialOf(view, ask.of).some((e) => e.name === ask.name);
      if (listed && ask.name !== null) {
        void toggleLook(ask.of, ask.name).then(() =>
          rowPart(ask.of, ask.name ?? "", "look-panel")?.scrollIntoView({ block: "nearest" }),
        );
      } else {
        void tick().then(() => {
          fields[ask.of]?.scrollIntoView({ block: "nearest" });
          fields[ask.of]?.focus();
        });
      }
    });
  });
</script>

<section class="vocabulary" aria-labelledby="vocab-title" data-vocabulary bind:this={section}>
  <h4 class="group" id="vocab-title">{t("projects.vocabulary.title")}</h4>
  <Card>
    <div class="body">
      <p class="intro">{t("projects.vocabulary.intro", { root })}</p>
      <p class="intro">{t("projects.vocabulary.keep")}</p>
      {#if problem !== null}<p class="warn" role="alert">{problem}</p>{/if}
      {#each KINDS as k (k.kind)}
        {@const list = officialOf(view, k.kind)}
        {@const clean = vocabularyName(typed[k.kind])}
        <div class="kind">
          <span class="kind-title" id="vocab-{k.kind}">{t(k.title)}</span>
          {#if list.length === 0}
            <p class="quiet">{t("projects.vocabulary.none")}</p>
          {:else}
            <ul class="names" aria-labelledby="vocab-{k.kind}">
              {#each list as entry (entry.name)}
                {@const Icon = isIcon(entry.icon) ? TAG_ICONS[entry.icon] : null}
                <li class="entry" data-official="{k.kind}:{entry.name}">
                  {#if is("rename", k.kind, entry.name)}
                    <form
                      class="rename"
                      onsubmit={(e) => {
                        e.preventDefault();
                        const input = rowPart(k.kind, entry.name, "new-name");
                        if (input instanceof HTMLInputElement)
                          void saveRename(k.kind, entry.name, input);
                      }}
                    >
                      <input
                        type="text"
                        class="field-input"
                        maxlength={40}
                        autocomplete="off"
                        spellcheck="false"
                        value={entry.name}
                        data-act="new-name"
                        aria-label={t("projects.vocabulary.newName", { name: entry.name })}
                        disabled={busy}
                        onkeydown={(e) => renameKey(e, k.kind, entry.name)}
                      />
                      <button type="submit" class="btn btn--sm btn--primary" disabled={busy}
                        >{t("projects.vocabulary.save")}</button
                      >
                      <button
                        type="button"
                        class="btn btn--sm"
                        disabled={busy}
                        onclick={() => void cancel(k.kind, entry.name, "rename")}
                        >{t("common.cancel")}</button
                      >
                    </form>
                  {:else if is("remove", k.kind, entry.name)}
                    <div
                      class="ask"
                      role="group"
                      aria-label={t("projects.vocabulary.removeNamed", { name: entry.name })}
                    >
                      <p>
                        {t(
                          k.kind === "tag"
                            ? "projects.vocabulary.removeAskTag"
                            : "projects.vocabulary.removeAskCategory",
                          { name: entry.name },
                        )}
                      </p>
                      <div class="buttons">
                        <button
                          type="button"
                          class="btn btn--sm btn--primary"
                          data-act="confirm-remove"
                          disabled={busy}
                          onkeydown={(e) => escape(e, k.kind, entry.name, "remove")}
                          onclick={() => void confirmRemove(k.kind, entry.name)}
                          >{t("projects.vocabulary.remove")}</button
                        >
                        <button
                          type="button"
                          class="btn btn--sm"
                          disabled={busy}
                          onkeydown={(e) => escape(e, k.kind, entry.name, "remove")}
                          onclick={() => void cancel(k.kind, entry.name, "remove")}
                          >{t("common.cancel")}</button
                        >
                      </div>
                    </div>
                  {:else}
                    <span
                      class="name"
                      class:tinted={isColor(entry.color)}
                      style:--c={isColor(entry.color) ? `var(--tag-${entry.color})` : null}
                      title={entry.name}
                      >{#if Icon}<Icon size={11} />{/if}<span class="name-text">{entry.name}</span
                      ></span
                    >
                    <span class="actions">
                      <button
                        type="button"
                        class="btn btn--sm"
                        data-act="rename"
                        aria-label={t("projects.vocabulary.renameNamed", { name: entry.name })}
                        disabled={blocked !== null}
                        title={blocked ?? undefined}
                        onclick={() => void startRename(k.kind, entry.name)}
                        >{t("projects.vocabulary.rename")}</button
                      >
                      <button
                        type="button"
                        class="btn btn--sm"
                        data-act="look"
                        aria-label={t("projects.vocabulary.lookNamed", { name: entry.name })}
                        aria-expanded={is("look", k.kind, entry.name)}
                        disabled={blocked !== null}
                        title={blocked ?? undefined}
                        onclick={() => void toggleLook(k.kind, entry.name)}
                        >{t("projects.vocabulary.look")}</button
                      >
                      <button
                        type="button"
                        class="btn btn--sm"
                        data-act="remove"
                        aria-label={t("projects.vocabulary.removeNamed", { name: entry.name })}
                        disabled={blocked !== null}
                        title={blocked ?? undefined}
                        onclick={() => void startRemove(k.kind, entry.name)}
                        >{t("projects.vocabulary.remove")}</button
                      >
                    </span>
                  {/if}
                  {#if is("look", k.kind, entry.name)}
                    <!-- Esc from any of its choices closes it (keys bubble from its buttons) -->
                    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
                    <div
                      class="look"
                      role="group"
                      aria-label={t("projects.vocabulary.lookNamed", { name: entry.name })}
                      data-act="look-panel"
                      onkeydown={(e) => escape(e, k.kind, entry.name, "look")}
                    >
                      <LookPicker
                        color={isColor(entry.color) ? entry.color : null}
                        icon={isIcon(entry.icon) ? entry.icon : null}
                        disabled={blocked !== null}
                        pick={(color, icon) => pickLook(k.kind, entry, color, icon)}
                      />
                      <div class="buttons">
                        <button
                          type="button"
                          class="btn btn--sm"
                          onclick={() => void cancel(k.kind, entry.name, "look")}
                          >{t("projects.vocabulary.done")}</button
                        >
                      </div>
                    </div>
                  {/if}
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
              aria-describedby="vocab-as-{k.kind} vocab-where vocab-said-{k.kind}"
              disabled={blocked !== null}
              bind:value={typed[k.kind]}
              bind:this={fields[k.kind]}
              oninput={() => {
                said[k.kind] = "";
                added[k.kind] = null;
              }}
            />
            <button
              type="submit"
              class="btn btn--sm"
              disabled={busy || clean === null || blocked !== null}
              title={blocked ?? (clean === null ? t("projects.vocabulary.typeName") : undefined)}
              >{t("projects.vocabulary.add")}</button
            >
          </form>
          <!-- how the typed name will be kept, before it is added (lowercase, dashes for spaces) -->
          <p class="quiet as" id="vocab-as-{k.kind}">
            {clean !== null && clean !== typed[k.kind].trim()
              ? t("projects.vocabulary.addAs", { name: clean })
              : ""}
          </p>
          <div class="said">
            <p class="quiet" id="vocab-said-{k.kind}" role="status">{said[k.kind]}</p>
            {#if added[k.kind] !== null}
              <button
                type="button"
                class="btn btn--sm"
                disabled={busy}
                onclick={() => void undoAdd(k.kind)}>{t("fileOps.undo")}</button
              >
            {/if}
          </div>
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
  .as:empty {
    display: none;
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
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }
  .entry {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1) var(--sp-2);
    min-width: 0;
    padding: 2px 0;
  }
  .name {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    min-width: 0;
    max-width: 100%;
    padding: 0 6px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: var(--surface-2);
    font-size: var(--fs-ui-xs);
    line-height: 18px;
    white-space: nowrap;
  }
  .name-text {
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name.tinted {
    border-color: color-mix(in srgb, var(--c) 28%, transparent);
    background: color-mix(in srgb, var(--c) 12%, transparent);
    color: var(--c);
  }
  .actions {
    display: inline-flex;
    flex-wrap: wrap;
    gap: var(--sp-1);
    margin-left: auto;
  }
  .rename,
  .add {
    display: flex;
    flex-wrap: wrap;
    gap: var(--sp-2);
    align-items: center;
  }
  .rename {
    flex: 1 1 100%;
  }
  .rename input,
  .add input {
    flex: 0 1 240px;
    min-width: 0;
  }
  .ask,
  .look {
    display: flex;
    flex: 1 1 100%;
    flex-direction: column;
    gap: var(--sp-1);
    padding: var(--sp-2);
    border: 1px solid var(--border);
    border-radius: var(--radius-control);
    background: var(--surface-2);
  }
  .ask p {
    overflow-wrap: anywhere;
  }
  .buttons,
  .said {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
  }
  .buttons {
    margin-top: var(--sp-1);
  }
  .hint {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-2);
  }
</style>
