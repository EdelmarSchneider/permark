<script lang="ts">
  // The tag editor (T): the file's one-line description and its tags. Tags from the front matter are locked; the app's
  // are added with Enter or a comma and removed with the cross or Backspace. Every change is saved as it is made.
  import { onMount, tick } from "svelte";
  import type { FileMeta } from "../../api";
  import { t } from "../../core/i18n";
  import TagChip from "./TagChip.svelte";
  import {
    MAX_DESCRIPTION,
    normalizeDescription,
    normalizeTag,
    normalizeTags,
    splitTyped,
    suggest,
  } from "./model";
  import { tags } from "./store.svelte";

  type Props = { path: string };
  let { path }: Props = $props();

  let root: HTMLElement | undefined = $state();
  let field: HTMLInputElement | undefined = $state();
  let meta = $state.raw<FileMeta | null>(null);
  let description = $state("");
  let typed = $state("");
  let active = $state(-1);

  const status = $derived(tags.status);
  const worktree = $derived(path.startsWith("@wt/"));
  const editable = $derived(meta?.editable === true);
  const mine = $derived(meta?.tags.filter((tag) => tag.app).map((tag) => tag.name) ?? []);
  const named = $derived(meta?.tags.map((tag) => tag.name) ?? []);
  const suggestions = $derived(suggest(tags.overview?.tags ?? [], typed, named));
  const name = $derived(path.slice(path.lastIndexOf("/") + 1));

  onMount(() => {
    const before = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    let gone = false;
    void tags.fileMeta(path).then(async (found) => {
      if (gone || found === null) return;
      meta = found;
      description = found.description;
      await tick();
      field?.focus();
    });
    const outside = (event: PointerEvent): void => {
      if (root && event.target instanceof Node && !root.contains(event.target)) void close();
    };
    // Escape closes the editor wherever the focus is inside the window
    const escape = (event: KeyboardEvent): void => {
      if (event.key !== "Escape" || event.isComposing) return;
      event.preventDefault();
      event.stopPropagation();
      void close();
    };
    window.addEventListener("pointerdown", outside, true);
    window.addEventListener("keydown", escape, true);
    return () => {
      gone = true;
      window.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("keydown", escape, true);
      before?.focus();
    };
  });

  async function save(next: readonly string[]): Promise<void> {
    const answer = await tags.setTags(path, next);
    if (answer !== null) meta = answer;
  }

  async function add(raw: string): Promise<void> {
    const tag = normalizeTag(raw);
    typed = "";
    active = -1;
    if (tag === null || mine.includes(tag) || named.includes(tag)) return;
    await save(normalizeTags([...mine, tag]));
  }

  async function remove(tag: string): Promise<void> {
    await save(mine.filter((n) => n !== tag));
    field?.focus();
  }

  async function commitDescription(): Promise<void> {
    if (!editable || normalizeDescription(description) === (meta?.description ?? "")) return;
    const answer = await tags.setDescription(path, description);
    if (answer !== null) {
      meta = answer;
      description = answer.description;
    }
  }

  async function close(): Promise<void> {
    await commitDescription();
    tags.closeEditor();
  }

  function oninput(): void {
    // a comma ends a tag
    const split = splitTyped(typed);
    if (split.done.length > 0) {
      typed = split.rest;
      void save(normalizeTags([...mine, ...split.done]));
    }
    active = -1;
  }

  function onkeydown(event: KeyboardEvent): void {
    if (event.isComposing || event.keyCode === 229) return;
    switch (event.key) {
      case "ArrowDown":
        if (suggestions.length === 0) break;
        event.preventDefault();
        active = (active + 1) % suggestions.length;
        break;
      case "ArrowUp":
        if (suggestions.length === 0) break;
        event.preventDefault();
        active = (active - 1 + suggestions.length) % suggestions.length;
        break;
      case "Enter": {
        event.preventDefault();
        const picked = suggestions[active];
        if (picked) void add(picked.name);
        else if (typed.trim() !== "") void add(typed);
        else void close();
        break;
      }
      case "Backspace": {
        const last = mine.at(-1);
        if (typed === "" && last !== undefined) {
          event.preventDefault();
          void remove(last);
        }
        break;
      }
      default:
        break;
    }
  }
</script>

<div
  class="pop"
  bind:this={root}
  role="dialog"
  aria-modal="false"
  aria-label={t("tags.popoverTitle")}
  tabindex="-1"
  {onkeydown}
>
  <header>
    <h2>{t("tags.popoverTitle")}</h2>
    <span class="file" title={path}>{name}</span>
  </header>

  <label class="label" for="tags-description">{t("tags.description")}</label>
  <div class="box">
    <input
      id="tags-description"
      type="text"
      value={description}
      maxlength={MAX_DESCRIPTION}
      placeholder={t("tags.descriptionPlaceholder")}
      disabled={!editable}
      oninput={(e) => (description = e.currentTarget.value)}
      onblur={() => void commitDescription()}
      onkeydown={(e) => {
        if (e.key === "Enter" && !e.isComposing) {
          e.preventDefault();
          e.stopPropagation();
          void commitDescription();
          field?.focus();
        }
      }}
    />
    <span class="counter"
      >{t("tags.descriptionCounter", { count: description.length, max: MAX_DESCRIPTION })}</span
    >
  </div>

  <span class="label" id="tags-label">{t("tags.tags")}</span>
  <div class="chips" role="group" aria-labelledby="tags-label">
    {#each meta?.tags ?? [] as tag (tag.name)}
      {@const style = tags.styleOf(tag.name)}
      {@const mark = tags.markOf(tag)}
      <TagChip
        name={tag.name}
        color={style?.color}
        icon={style?.icon}
        status={mark?.status ?? null}
        statusWords={mark?.words ?? ""}
        locked={tag.file}
        onremove={tag.app && !tag.file && editable ? () => void remove(tag.name) : undefined}
      />
    {/each}
    <input
      bind:this={field}
      class="field"
      type="text"
      bind:value={typed}
      {oninput}
      disabled={!editable}
      placeholder={t("tags.inputPlaceholder")}
      aria-label={t("tags.inputLabel")}
      autocomplete="off"
      autocapitalize="off"
      spellcheck="false"
    />
  </div>
  {#if typed.trim() !== ""}
    <p class="hint">{t("tags.enterHint")}</p>
  {/if}

  {#if suggestions.length > 0 && editable}
    <ul class="suggestions" aria-label={t("tags.suggestions")}>
      {#each suggestions as suggestion, at (suggestion.name)}
        {@const style = tags.styleOf(suggestion.name)}
        <li>
          <button
            type="button"
            class:active={active === at}
            onclick={() => void add(suggestion.name)}
            onpointermove={() => (active = at)}
          >
            <TagChip name={suggestion.name} color={style?.color} icon={style?.icon} />
            <span class="n">{suggestion.count}</span>
          </button>
        </li>
      {/each}
    </ul>
  {/if}

  <footer>
    {#if worktree}
      <p class="note">{t("tags.worktreeReadOnly")}</p>
    {:else if status?.problem}
      <p class="note warn" role="alert">
        {t("tags.readOnly")}
        {status.problem.line !== null
          ? t("tags.metaUnreadableLine", { line: status.problem.line })
          : ""}
      </p>
    {:else if status}
      <p class="note">
        {status.location === "project" ? t("tags.savedProject") : t("tags.savedApp")}
      </p>
    {/if}
    <button type="button" class="btn btn--sm btn--primary" onclick={() => void close()}>
      {t("tags.done")}
    </button>
  </footer>
</div>

<style>
  .pop {
    position: fixed;
    top: 84px;
    right: var(--sp-5);
    z-index: 40;
    display: flex;
    flex-direction: column;
    gap: var(--sp-1);
    width: min(380px, calc(100vw - 2 * var(--sp-5)));
    max-height: calc(100vh - 120px);
    overflow-y: auto;
    padding: var(--sp-3);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-card);
    background: var(--surface);
    box-shadow: var(--shadow-pop);
    font-size: var(--fs-ui);
  }
  header {
    display: flex;
    align-items: baseline;
    gap: var(--sp-2);
  }
  h2 {
    margin: 0;
    font-size: var(--fs-ui);
    font-weight: 600;
  }
  .file {
    min-width: 0;
    overflow: hidden;
    color: var(--muted);
    font-size: var(--fs-ui-sm);
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .label {
    margin-top: var(--sp-2);
    color: var(--muted);
    font-size: var(--fs-ui-sm);
  }
  .box {
    display: flex;
    align-items: center;
    gap: var(--sp-2);
  }
  input[type="text"] {
    min-width: 0;
    padding: 5px var(--sp-2);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .box input {
    flex: 1;
  }
  input:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  input:disabled {
    opacity: 0.6;
  }
  .counter {
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    font-variant-numeric: tabular-nums;
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--sp-1);
    padding: var(--sp-1);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-control);
    background: var(--bg);
  }
  .chips .field {
    flex: 1;
    min-width: 90px;
    padding: 2px 4px;
    border: 0;
    background: transparent;
  }
  .chips .field:focus-visible {
    outline: none;
  }
  .chips:focus-within {
    outline: 2px solid var(--accent);
    outline-offset: -1px;
  }
  .hint,
  .note {
    margin: 0;
    color: var(--muted);
    font-size: var(--fs-ui-xs);
  }
  .warn {
    color: var(--git-deleted-text);
  }
  .suggestions {
    margin: 0;
    padding: var(--sp-1) 0;
    list-style: none;
    border-top: 1px solid var(--border);
  }
  .suggestions button {
    display: flex;
    align-items: center;
    justify-content: space-between;
    width: 100%;
    padding: 3px var(--sp-1);
    border: 0;
    border-radius: var(--radius-control);
    background: transparent;
    cursor: pointer;
  }
  .suggestions button.active,
  .suggestions button:hover {
    background: var(--hover);
  }
  .n {
    color: var(--faint);
    font-size: var(--fs-ui-xs);
    font-variant-numeric: tabular-nums;
  }
  footer {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-2);
    margin-top: var(--sp-2);
  }
</style>
