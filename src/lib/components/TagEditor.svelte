<script lang="ts">
  import { onDestroy, onMount, tick, untrack } from "svelte";
  import { t } from "../i18n";
  import { DescriptionDraft } from "../descriptionDraft";
  import { splitWorktree } from "../paths";
  import { suggest, type Suggestion } from "../tagModel";
  import {
    charCount,
    MAX_DESCRIPTION_CHARS,
    normalizeTag,
    normalizeTags,
    splitTagInput,
  } from "../tagText";
  import { docs } from "../stores/docs.svelte";
  import { tags } from "../stores/tags.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import Popover, { type Anchor } from "./Popover.svelte";
  import TagChip from "./TagChip.svelte";
  import TagSuggestions from "./TagSuggestions.svelte";

  const DESCRIPTION_DELAY_MS = 300;

  // next to the [+] that opened it, else under the meta strip, else under the document header
  function findAnchor(): Anchor {
    const strip = document.querySelector<HTMLElement>(".meta-strip");
    if (strip) {
      const r = strip.getBoundingClientRect();
      return { left: r.left, right: r.left, top: r.bottom, bottom: r.bottom };
    }
    const header = document.querySelector<HTMLElement>(".doc-header");
    const r = header?.getBoundingClientRect();
    const left = (r?.left ?? 0) + 24;
    const bottom = r?.bottom ?? 44;
    return { left, right: left, top: bottom, bottom };
  }
  const anchor = untrack(() => ui.tagEditor?.anchor ?? findAnchor());

  // The editor belongs to the document it was opened on: every change is saved to that one, even
  // if the reader navigates away while it is open (then it closes).
  const path = untrack(() => docs.path ?? "");
  const meta = $derived(tags.meta && tags.meta.path === path ? tags.meta : null);
  const writable = $derived(meta?.writable === true);
  const appTags = $derived(meta ? meta.tags.filter((tag) => tag.app).map((tag) => tag.name) : []);
  const onFile = $derived(new Set(meta ? meta.tags.map((tag) => tag.name) : []));
  const status = $derived(tags.overview?.status ?? null);

  let description = $state(untrack(() => meta?.description ?? ""));
  /** What is typed is saved through this: always to the document the editor was opened on. */
  const draft = new DescriptionDraft(
    path,
    untrack(() => meta?.description ?? ""),
    (target, text) => {
      void tags.setDescription(target, text);
    },
  );
  let canWrite = untrack(() => meta?.writable === true);
  let descriptionFocused = $state(false);
  let typed = $state("");
  let active = $state(0);
  let open = $state(false);
  let focused = $state(false);
  let input: HTMLInputElement | undefined = $state();
  let descriptionInput: HTMLInputElement | undefined = $state();
  let saveTimer: ReturnType<typeof setTimeout> | null = null;

  const rows = $derived<Suggestion[]>(
    focused && (typed.trim() !== "" || open) ? suggest(typed, tags.counts, onFile) : [],
  );
  const full = $derived(charCount(description) >= MAX_DESCRIPTION_CHARS);
  const where = $derived(
    vault.stateLocation === "project" ? t("tags.savedProject") : t("tags.savedApp"),
  );
  const readOnlyNote = $derived(
    splitWorktree(path).worktree !== null ? t("tags.worktreeReadOnly") : t("tags.readOnly"),
  );

  // another change of the description (an agent edited meta.toml) shows up while the box is not being typed in
  $effect(() => {
    const incoming = meta?.description;
    const writable = meta?.writable;
    untrack(() => {
      if (incoming === undefined) return; // another document's tags are showing: leave the box alone
      canWrite = writable === true;
      if (!descriptionFocused && saveTimer === null) {
        description = incoming;
        draft.adopt(incoming);
      }
    });
  });

  $effect(() => {
    void rows.length;
    untrack(() => {
      if (active >= rows.length) active = Math.max(0, rows.length - 1);
    });
  });

  // the reader moved to another document (back, forward, a link) or it went away: what was typed is
  // saved to the document the editor was opened on, and the editor closes
  $effect(() => {
    if (docs.path === path) return;
    untrack(() => {
      saveDescription();
      close();
    });
  });

  function close(): void {
    ui.tagEditor = null;
  }

  function saveDescription(): void {
    if (saveTimer !== null) clearTimeout(saveTimer);
    saveTimer = null;
    draft.flush(description, canWrite);
  }

  /** Keeps what is typed within the limit, counting characters like the backend does. */
  function capDescription(): void {
    const chars = Array.from(description);
    if (chars.length > MAX_DESCRIPTION_CHARS) {
      description = chars.slice(0, MAX_DESCRIPTION_CHARS).join("");
    }
  }

  function scheduleDescription(): void {
    if (saveTimer !== null) clearTimeout(saveTimer);
    saveTimer = setTimeout(saveDescription, DESCRIPTION_DELAY_MS);
  }

  function setAppTags(next: string[]): void {
    if (writable) void tags.setTags(path, next);
  }

  function add(names: string[]): void {
    const fresh = normalizeTags(names).filter((name) => !onFile.has(name));
    if (fresh.length > 0) setAppTags([...appTags, ...fresh]);
  }

  function remove(name: string): void {
    setAppTags(appTags.filter((n) => n !== name));
  }

  function pick(row: Suggestion | undefined): void {
    if (!row) return;
    add([row.type === "tag" ? row.match.tag.name : row.name]);
    typed = "";
    open = false;
    active = 0;
  }

  function oninput(): void {
    const { done, rest } = splitTagInput(typed);
    if (done.length > 0) {
      add(done);
      typed = rest.trimStart();
    }
    active = 0;
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (rows.length === 0) {
        open = true;
        return;
      }
      active = (active + (e.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length;
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (rows.length > 0 && (typed.trim() !== "" || open)) pick(rows[active]);
      else if (typed.trim() !== "") {
        const name = normalizeTag(typed);
        if (name) add([name]);
        typed = "";
      }
    } else if (e.key === "Backspace" && typed === "") {
      const last = [...(meta?.tags ?? [])].reverse().find((tag) => tag.app && !tag.file);
      if (last) {
        e.preventDefault();
        remove(last.name);
      }
    }
  }

  onMount(() => {
    void tags.refreshOverview();
    void tick().then(() => (writable ? input : descriptionInput)?.focus());
  });

  onDestroy(saveDescription);
</script>

<Popover {anchor} variant="panel" wide label={t("tags.popoverTitle")} onclose={close}>
  <div class="tag-editor">
    <div class="field">
      <div class="field__row">
        <label class="field__label" for="tag-description">{t("tags.description")}</label>
        <span class={["tag-editor__counter", full && "is-full"]} aria-live="off">
          {t("tags.descriptionCounter", {
            count: charCount(description),
            max: MAX_DESCRIPTION_CHARS,
          })}
        </span>
      </div>
      <input
        id="tag-description"
        bind:this={descriptionInput}
        class="text-input"
        bind:value={description}
        placeholder={t("tags.descriptionPlaceholder")}
        disabled={!writable}
        spellcheck="false"
        autocomplete="off"
        onfocus={() => (descriptionFocused = true)}
        onblur={() => {
          descriptionFocused = false;
          saveDescription();
        }}
        oninput={() => {
          capDescription();
          scheduleDescription();
        }}
        onkeydown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            saveDescription();
            input?.focus();
          }
        }}
      />
    </div>

    <div class="field">
      <span class="field__label" id="tag-editor-tags">{t("tags.tags")}</span>
      <!-- clicking the empty part of the box puts the cursor in it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
      <div
        class="chip-input"
        role="group"
        aria-labelledby="tag-editor-tags"
        onclick={(e) => e.target === e.currentTarget && input?.focus()}
      >
        {#each meta?.tags ?? [] as tag (tag.name)}
          <TagChip
            name={tag.name}
            file={tag.file}
            removable={writable && tag.app && !tag.file}
            onremove={() => remove(tag.name)}
          />
        {/each}
        <input
          bind:this={input}
          bind:value={typed}
          class="chip-input__field"
          role="combobox"
          aria-label={t("tags.inputLabel")}
          aria-expanded={rows.length > 0}
          aria-controls="tag-editor-suggestions"
          aria-activedescendant={rows.length > 0 ? `tag-editor-suggestions-${active}` : undefined}
          aria-autocomplete="list"
          placeholder={t("tags.inputPlaceholder")}
          disabled={!writable}
          spellcheck="false"
          autocomplete="off"
          onfocus={() => (focused = true)}
          onblur={() => {
            focused = false;
            open = false;
          }}
          {oninput}
          {onkeydown}
        />
      </div>
      {#if rows.length > 0}
        <TagSuggestions
          id="tag-editor-suggestions"
          {rows}
          {active}
          onpick={pick}
          onhover={(i) => (active = i)}
        />
      {/if}
    </div>

    {#if !writable && meta}
      <div class="tag-editor__note" role="status">{readOnlyNote}</div>
    {/if}

    <div class="tag-editor__foot">
      {#if status?.writeError}
        <span class="tag-editor__error" role="alert">
          {t("tags.writeError", { message: status.writeError })}
        </span>
      {:else}
        <span>{where}</span>
      {/if}
      <button type="button" class="btn btn--sm btn--primary" onclick={close}>
        {t("tags.done")}
      </button>
    </div>
  </div>
</Popover>
