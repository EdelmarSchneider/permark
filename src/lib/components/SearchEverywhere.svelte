<script lang="ts">
  import { onMount, tick, untrack } from "svelte";
  import { api } from "../api";
  import { bridge } from "../bridge";
  import { availableCommands, matchCommands, type CommandContext } from "../commands";
  import { errorMessage } from "../errors";
  import { gitStateLabel } from "../gitLabels";
  import { keys, t, tn, type MessageKey } from "../i18n";
  import { en } from "../i18n/en";
  import { basename, dirname, isMarkdown, splitWorktree } from "../paths";
  import { registerOverlay, rememberFocus } from "../overlays";
  import { quickMatches, type QuickMatches } from "../quickOpenModel";
  import {
    allowedScope,
    applyPrefix,
    buildView,
    commandRows,
    fileRows,
    moveActive,
    outlineRows,
    RESULT_LIMIT,
    recentFileRows,
    scopesFor,
    sectionRows,
    stepScope,
    tagRows,
    type FileRow,
    type GroupTitle,
    type Row,
    type Scope,
  } from "../searchEverywhereModel";
  import { isMacPlatform, shortcutLabel } from "../shortcuts";
  import { knownTagsHint } from "../tagModel";
  import { unknownTags } from "../tagQuery";
  import { TagQueryState } from "../tagQueryState.svelte";
  import { highlightSegments } from "../text";
  import type { SearchHit } from "../types";
  import { agent } from "../stores/agent.svelte";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { distribution } from "../stores/distribution.svelte";
  import { docs } from "../stores/docs.svelte";
  import { edit } from "../stores/edit.svelte";
  import { feed } from "../stores/feed.svelte";
  import { filter } from "../stores/filter.svelte";
  import { outline } from "../stores/outline.svelte";
  import { tags } from "../stores/tags.svelte";
  import { ui } from "../stores/ui.svelte";
  import { unread } from "../stores/unread.svelte";
  import { vault } from "../stores/vault.svelte";
  import { relativeTime } from "../time";
  import { badgeFor } from "../tree";
  import Icon from "./Icon.svelte";
  import TagChip from "./TagChip.svelte";
  import TagSuggestions from "./TagSuggestions.svelte";
  import UnreadDot from "./UnreadDot.svelte";

  /** The search index is asked after this pause in typing. */
  const SECTIONS_DEBOUNCE_MS = 80;
  /** With a filter after the search (tags, the Explorer filter) the index is asked for more than is shown. */
  const FILTERED_LIMIT = 200;

  const SCOPE_NAME = {
    all: "everywhere.scope.all",
    files: "everywhere.scope.files",
    sections: "everywhere.scope.sections",
    tags: "everywhere.scope.tags",
    commands: "everywhere.scope.commands",
  } as const satisfies Record<Scope, MessageKey>;
  const PLACEHOLDER = {
    all: "everywhere.placeholder.all",
    files: "quick.placeholder",
    sections: "everywhere.placeholder.sections",
    tags: "everywhere.placeholder.tags",
    commands: "everywhere.placeholder.commands",
  } as const satisfies Record<Scope, MessageKey>;
  const GROUP_NAME = {
    changed: "quick.recentChanged",
    opened: "quick.recentOpened",
    files: SCOPE_NAME.files,
    sections: SCOPE_NAME.sections,
    tags: SCOPE_NAME.tags,
    commands: SCOPE_NAME.commands,
  } as const satisfies Record<GroupTitle, MessageKey>;

  const scopes = $derived(scopesFor(!vault.isSimple));
  let scope = $state<Scope>(
    allowedScope(scopesFor(!vault.isSimple), ui.searchBox?.scope ?? ui.searchScope),
  );

  /** The words and the `tag:` filters of the box. */
  const q = new TagQueryState(() => tags.counts);
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let list: HTMLElement | undefined = $state();

  /** The "+N hidden files match" row was used: the files the Explorer filter hides are listed too. */
  let revealHidden = $state(false);

  // The shortcut that opened the box (or pressed again while it is open) names the scope.
  $effect(() => {
    const asked = ui.searchBox?.scope;
    if (asked === undefined) return;
    untrack(() => {
      scope = allowedScope(scopes, asked);
    });
  });

  // ---- what is typed ---------------------------------------------------------------------------------

  const idle = $derived(q.words === "" && q.chips.length === 0);
  const hasWords = $derived(q.words !== "");
  /** The query as the "nothing matches" messages quote it. */
  const typed = $derived(q.words || q.text.trim());

  /** The files the box may list. */
  const files = $derived(revealHidden ? vault.files : filter.files);
  const describe = (path: string): string => tags.index.get(path)?.description ?? "";

  // chips of tags no file has, and a `tag:x` being typed that nothing matches
  const unknown = $derived([
    ...unknownTags(q.chips, tags.known),
    ...(q.partial !== null && q.partial !== "" && q.suggestions.length === 0 ? [q.partial] : []),
  ]);
  const suggesting = $derived(q.partial !== null && q.suggestions.length > 0);

  // ---- files -----------------------------------------------------------------------------------------

  const recency = $derived.by(() => {
    const map = new Map<string, number>();
    docs.recentPaths().forEach((p, i) => map.set(p, i === 0 ? 1 : 0));
    for (const e of feed.entries) if (e.type === "file") map.set(e.path, e.atMs);
    return map;
  });

  // tag-only queries list the files by recency: the time a file changed counts too
  const listRecency = $derived.by(() => {
    const map = new Map(recency);
    for (const entry of tags.index.values()) {
      map.set(entry.path, Math.max(map.get(entry.path) ?? 0, entry.modifiedMs));
    }
    return map;
  });

  /** When each file last changed, newest entry of the feed first. */
  const changedAt = $derived.by(() => {
    const map = new Map<string, number>();
    for (const e of feed.entries)
      if (e.type === "file" && !map.has(e.path)) map.set(e.path, e.atMs);
    return map;
  });

  /** What the file search finds. When only files the Explorer filter hides match, they are listed at once. */
  const matches = $derived.by<QuickMatches | null>(() => {
    if ((scope !== "all" && scope !== "files") || idle || unknown.length > 0) return null;
    return quickMatches({
      text: q.words,
      tags: q.chips,
      files: filter.files,
      hiddenFiles: filter.hiddenFiles,
      allFiles: vault.files,
      reveal: revealHidden,
      tagsOf: (path) => tags.index.get(path)?.tags,
      describe,
      recency: listRecency,
      limit: RESULT_LIMIT,
    });
  });

  const fileList = $derived.by<FileRow[]>(() => {
    if (scope !== "all" && scope !== "files") return [];
    if (idle) {
      const changed: { path: string; atMs: number }[] = [];
      for (const e of feed.entries) {
        if (e.type === "file" && e.isDoc && e.kind !== "removed") {
          changed.push({ path: e.path, atMs: e.atMs });
        }
      }
      return recentFileRows({
        changed,
        opened: docs.recentPaths(),
        exists: new Set(files),
        describe,
      });
    }
    return fileRows(matches?.found ?? [], describe, changedAt);
  });
  const hiddenCount = $derived(matches?.moreHidden ?? 0);

  // ---- sections --------------------------------------------------------------------------------------

  let sectionHits = $state.raw<SearchHit[]>([]);
  /** The words `sectionHits` answer. */
  let sectionsFor = $state("");
  let sectionsError = $state<string | null>(null);
  let sectionsSeq = 0;

  const wantsSections = $derived((scope === "all" || scope === "sections") && hasWords);
  /** Hits are filtered after the search, so the index is asked for more. */
  const filtered = $derived(q.chips.length > 0 || filter.hiddenFiles.length > 0);

  $effect(() => {
    const words = q.words;
    const wanted = wantsSections;
    const limit = filtered ? FILTERED_LIMIT : RESULT_LIMIT;
    const seq = ++sectionsSeq;
    if (!wanted) {
      sectionHits = [];
      sectionsFor = "";
      sectionsError = null;
      return;
    }
    const timer = setTimeout(async () => {
      try {
        const found = await api.search(words, limit);
        if (seq !== sectionsSeq) return;
        sectionHits = found;
        sectionsFor = words;
        sectionsError = null;
      } catch (error) {
        if (seq !== sectionsSeq) return;
        sectionsError = errorMessage(error);
      }
    }, SECTIONS_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  });

  /** The index has not answered for the words in the box yet. */
  const sectionsPending = $derived(
    wantsSections && sectionsFor !== q.words && sectionsError === null,
  );
  const indexing = $derived(vault.indexProgress !== null);

  const sectionList = $derived.by(() => {
    if (scope !== "all" && scope !== "sections") return [];
    if (idle) {
      // before anything is typed: the outline of the open document
      return scope === "sections" && outline.path === docs.path && docs.path !== null
        ? outlineRows(docs.path, outline.model.items, t("outline.untitled"))
        : [];
    }
    if (!hasWords || unknown.length > 0) return [];
    return sectionRows({
      hits: sectionHits,
      visible: new Set(files),
      tags: q.chips,
      tagsOf: (path) => tags.index.get(path)?.tags,
      limit: RESULT_LIMIT,
    });
  });

  // ---- tags and commands -----------------------------------------------------------------------------

  const tagList = $derived(
    scope === "tags" || (scope === "all" && hasWords) ? tagRows(q.words, tags.counts, q.chips) : [],
  );

  const mac = typeof navigator !== "undefined" && isMacPlatform(navigator.userAgent);
  const keyText = (combo: string): string => shortcutLabel(combo, mac, keys);

  const context = $derived<CommandContext>({
    ide: !vault.isSimple,
    hasDoc: docs.current !== null,
    editing: edit.active,
    canBack: docs.canBack,
    canForward: docs.canForward,
    hasChanges: docs.marks.changed + docs.marks.removed > 0,
    stateInProject: vault.stateLocation === "project",
    hasRepo: !!vault.git?.isRepo && !vault.git.gitError,
    hasUnread: unread.count > 0,
    agentOn: agent.enabled,
    storeBuild: distribution.isStore,
  });

  const commandList = $derived(
    scope === "commands" || (scope === "all" && hasWords)
      ? commandRows(
          matchCommands(q.words, availableCommands(context), {
            current: (command) => t(command.label),
            english: (command) => en[command.label],
          }),
          keyText,
        )
      : [],
  );

  // ---- the list --------------------------------------------------------------------------------------

  const view = $derived(
    buildView({
      scope,
      idle,
      hasWords,
      files: fileList,
      moreHidden: hiddenCount,
      sections: sectionList,
      tags: tagList,
      commands: commandList,
    }),
  );
  const rows = $derived(view.rows);
  const positionOf = $derived(new Map(rows.map((row, i) => [row.key, i])));

  const emptyText = $derived.by(() => {
    if (scope === "commands") return t("everywhere.noCommands", { query: typed });
    if (scope === "tags") {
      return tags.counts.length === 0 ? t("tags.empty") : t("everywhere.noTags", { query: typed });
    }
    if (unknown.length > 0) {
      return tags.counts.length === 0
        ? t("tags.noTagsAtAll", { name: unknown[0] })
        : t("tags.unknownTag", { name: unknown[0], tags: knownTagsHint(tags.counts) });
    }
    if (scope === "sections") {
      if (idle) return t("everywhere.sectionsHint");
      if (!hasWords) return t("search.tagsOnly");
      if (sectionsPending) return t("search.searching");
      if (sectionsError !== null) return t("search.error", { message: sectionsError });
      return indexing ? t("search.noneYet") : t("everywhere.noSections", { query: typed });
    }
    if (idle) return t("quick.empty");
    if (!hasWords) return t("quick.tagsOnly");
    if (sectionsPending) return t("search.searching");
    return scope === "all"
      ? t("everywhere.none", { query: typed })
      : t("quick.none", { query: typed });
  });

  // ---- acting ----------------------------------------------------------------------------------------

  function close(): void {
    ui.closeSearch();
  }

  function reveal(): void {
    revealHidden = true;
    active = 0;
    input?.focus();
  }

  /** A scope picked on purpose (Tab, a click) is the one Ctrl+T comes back to. */
  function switchScope(next: Scope, remember = true): void {
    scope = next;
    if (remember) ui.searchScope = next;
    active = 0;
    input?.focus();
  }

  /** A tag becomes a filter and the box goes to the files that have it. */
  function pickTag(name: string): void {
    q.pick(name);
    q.setText("");
    switchScope("files", false);
  }

  function choose(row: Row | undefined): void {
    if (!row) return;
    switch (row.type) {
      case "file":
        close();
        void (row.ms !== null && row.group === "changed"
          ? app.openChanged(row.path)
          : app.openDoc(row.path));
        return;
      case "section": {
        close();
        const { hit, outline: heading } = row;
        if (hit) void app.openHit(hit, sectionsFor);
        else if (heading !== null) bridge.outline?.goTo(heading);
        return;
      }
      case "tag":
        pickTag(row.name);
        return;
      case "command": {
        close();
        const id = row.id;
        // after the box has gone and the focus is back where it was, so a command that opens something keeps its focus
        void tick().then(() => app.run(id));
        return;
      }
      case "more":
        reveal();
        return;
    }
  }

  function onkeydown(e: KeyboardEvent): void {
    // Up and Down in the tag suggestions, Enter or Tab to take one, Backspace to drop the last chip
    if (q.handleKey(e)) {
      e.preventDefault();
      return;
    }
    if (e.key === "Tab" && !e.ctrlKey && !e.altKey && !e.metaKey) {
      e.preventDefault();
      if (scopes.length > 1) switchScope(stepScope(scopes, scope, e.shiftKey ? -1 : 1));
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (rows.length === 0) return;
      active = moveActive(active, e.key === "ArrowDown" ? 1 : -1, rows.length);
      void tick().then(() =>
        list?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" }),
      );
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(rows[active]);
    }
  }

  function oninput(e: Event & { currentTarget: HTMLInputElement }): void {
    const raw = e.currentTarget.value;
    // `>` and `#` as the first character pick the scope and are not part of the query
    const next = applyPrefix(raw, scope, scopes);
    if (next.scope !== scope) {
      scope = next.scope;
      e.currentTarget.value = next.text;
    }
    q.setText(next.text);
  }

  // a new query or scope starts at the first row
  $effect(() => {
    void q.words;
    void q.chips;
    void scope;
    active = 0;
    void tick().then(() => list?.scrollTo({ top: 0 }));
  });

  onMount(() => {
    const restore = rememberFocus();
    input?.focus();
    const unregister = registerOverlay(close);
    return () => {
      unregister();
      restore();
    };
  });

  // ---- drawing ---------------------------------------------------------------------------------------

  function nameSegments(row: FileRow) {
    const start = row.path.lastIndexOf("/") + 1;
    return highlightSegments(
      basename(row.path),
      row.positions.filter((p) => p >= start).map((p) => p - start),
    );
  }

  function descriptionSegments(row: FileRow) {
    return highlightSegments(row.description, row.descriptionPositions);
  }

  function folderSegments(row: FileRow) {
    const { worktree, rel } = splitWorktree(row.path);
    const dir = dirname(rel);
    if (worktree) {
      return [{ text: `@wt ${worktree}${dir ? ` / ${dir}` : ""}`, match: false }];
    }
    const full = dirname(row.path);
    return highlightSegments(
      dir || t("feed.root"),
      full ? row.positions.filter((p) => p < full.length) : [],
    );
  }
</script>

{#snippet marked(text: string, positions: readonly number[])}
  {#each highlightSegments(text, positions) as seg, s (s)}{#if seg.match}<b>{seg.text}</b
      >{:else}{seg.text}{/if}{/each}
{/snippet}

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="overlay-top"
  onclick={(e) => {
    if (e.target === e.currentTarget) close();
  }}
>
  <div class="palette" role="dialog" aria-modal="true" aria-label={t("everywhere.label")}>
    <div class="palette__input">
      <Icon name="search" size="sm" />
      {#if q.chips.length > 0}
        <div class="palette__chips">
          {#each q.chips as chip (chip)}
            <TagChip
              name={chip}
              removable
              unknown={!tags.known.has(chip)}
              title={t("tags.queryChip", { name: chip })}
              removeLabel={t("tags.removeFilter", { name: chip })}
              onremove={() => {
                q.remove(chip);
                input?.focus();
              }}
            />
          {/each}
        </div>
      {/if}
      <input
        bind:this={input}
        value={q.text}
        {oninput}
        {onkeydown}
        placeholder={q.chips.length > 0 ? "" : t(PLACEHOLDER[scope])}
        aria-label={t("everywhere.label")}
        role="combobox"
        aria-expanded="true"
        aria-controls={suggesting ? "se-tags" : "se-list"}
        aria-activedescendant={suggesting
          ? `se-tags-${q.active}`
          : rows[active]
            ? `se-row-${active}`
            : undefined}
        spellcheck="false"
        autocomplete="off"
      />
    </div>
    {#if scopes.length > 1}
      <div class="palette__scopes" role="tablist" aria-label={t("everywhere.scopes")}>
        {#each scopes as s (s)}
          <button
            type="button"
            role="tab"
            class={["palette__scope", s === scope && "is-active"]}
            aria-selected={s === scope}
            tabindex="-1"
            onmousedown={(e) => e.preventDefault()}
            onclick={() => switchScope(s)}
          >
            {t(SCOPE_NAME[s])}
          </button>
        {/each}
      </div>
    {/if}
    {#if suggesting}
      <div class="palette__list">
        <TagSuggestions
          id="se-tags"
          rows={q.suggestions.map((match) => ({ type: "tag" as const, match }))}
          active={q.active}
          onpick={(row) => row.type === "tag" && q.pick(row.match.tag.name)}
          onhover={(i) => (q.active = i)}
        />
      </div>
    {:else}
      <div bind:this={list} id="se-list" class="palette__list" role="listbox">
        {#each view.groups as group (group.id)}
          {#if group.title}
            <div class="palette__group">
              <span>{t(GROUP_NAME[group.title])}</span>
              {#if group.showAll}
                {@const target = group.showAll}
                <button
                  type="button"
                  class="link-btn palette__showall"
                  tabindex="-1"
                  onmousedown={(e) => e.preventDefault()}
                  onclick={() => switchScope(target)}
                >
                  {t("everywhere.showAll", { n: group.total })}
                </button>
              {/if}
            </div>
          {/if}
          {#each group.rows as row (row.key)}
            {@const i = positionOf.get(row.key) ?? 0}
            <!-- svelte-ignore a11y_click_events_have_key_events -->
            <div
              id="se-row-{i}"
              class={[
                "palette__row",
                i === active && "is-active",
                row.type === "file" && row.hidden && "is-hidden",
                row.type === "more" && "palette__row--more",
              ]}
              role="option"
              tabindex="-1"
              aria-selected={i === active}
              data-i={i}
              onmousemove={() => (active = i)}
              onclick={() => choose(row)}
            >
              {#if row.type === "file"}
                {@const badge = badgeFor(vault.git?.files[row.path]?.state)}
                <Icon name={isMarkdown(row.path) ? "file-text" : "file"} size="sm" />
                <div class="palette__body">
                  <div class="palette__line">
                    {#if unread.isUnread(row.path)}<UnreadDot />{/if}
                    <span class="palette__name">
                      {#each nameSegments(row) as seg, s (s)}{#if seg.match}<b>{seg.text}</b
                          >{:else}{seg.text}{/if}{/each}
                    </span>
                    <span class="palette__path">
                      {#each folderSegments(row) as seg, s (s)}{#if seg.match}<b>{seg.text}</b
                          >{:else}{seg.text}{/if}{/each}
                    </span>
                    {#if row.ms !== null}<span class="palette__time"
                        >{relativeTime(row.ms, clock.now)}</span
                      >{/if}
                    {#if badge}
                      <span class={["badge", badge.className]} title={gitStateLabel(badge.state)}
                        >{badge.letter}</span
                      >
                    {/if}
                  </div>
                  {#if row.description !== ""}
                    <div class="palette__desc">
                      {#each descriptionSegments(row) as seg, s (s)}{#if seg.match}<b>{seg.text}</b
                          >{:else}{seg.text}{/if}{/each}
                    </div>
                  {/if}
                </div>
              {:else if row.type === "section"}
                <Icon name="heading" size="sm" />
                <div class="palette__body">
                  <div class="palette__line">
                    <span class="palette__name">{row.title}</span>
                    <span class="palette__path">{row.where}</span>
                  </div>
                  {#if row.snippet}
                    <div class="palette__desc">{@html row.snippet}</div>
                  {/if}
                </div>
              {:else if row.type === "tag"}
                <Icon name="hash" size="sm" />
                <div class="palette__body">
                  <div class="palette__line">
                    <span class="palette__name">{@render marked(row.name, row.positions)}</span>
                    <span class="palette__path">{tn("tags.filesCount", row.count)}</span>
                  </div>
                </div>
              {:else if row.type === "command"}
                <Icon name="command" size="sm" />
                <div class="palette__body">
                  <div class="palette__line">
                    <span class="palette__name">{@render marked(row.label, row.positions)}</span>
                    <span class="palette__path">{row.alias ?? ""}</span>
                    {#if row.shortcut}<kbd class="palette__keys">{row.shortcut}</kbd>{/if}
                  </div>
                </div>
              {:else}
                <Icon name="eye" size="sm" />
                <span>{tn("filter.hiddenMatches", row.count)}</span>
              {/if}
            </div>
          {/each}
        {:else}
          <div class="palette__empty">{emptyText}</div>
        {/each}
      </div>
    {/if}
    <div class="palette__foot">
      <span>{t("quick.footMove")}</span>
      <span>{scope === "commands" ? t("everywhere.footRun") : t("quick.footOpen")}</span>
      {#if scopes.length > 1}<span>{t("everywhere.footScope")}</span>{/if}
      <span>{t("quick.footClose")}</span>
    </div>
  </div>
</div>
