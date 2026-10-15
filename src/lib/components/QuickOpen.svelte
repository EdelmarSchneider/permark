<script lang="ts">
  import { onMount, tick } from "svelte";
  import { gitStateLabel } from "../gitLabels";
  import { t, tn } from "../i18n";
  import { basename, dirname, isMarkdown, splitWorktree } from "../paths";
  import { registerOverlay, rememberFocus } from "../overlays";
  import { onMoreRow, quickMatches, type QuickMatches } from "../quickOpenModel";
  import { knownTagsHint } from "../tagModel";
  import { unknownTags } from "../tagQuery";
  import { TagQueryState } from "../tagQueryState.svelte";
  import { highlightSegments } from "../text";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { docs } from "../stores/docs.svelte";
  import { feed } from "../stores/feed.svelte";
  import { filter } from "../stores/filter.svelte";
  import { tags } from "../stores/tags.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import { relativeTime } from "../time";
  import { badgeFor } from "../tree";
  import Icon from "./Icon.svelte";
  import TagChip from "./TagChip.svelte";
  import TagSuggestions from "./TagSuggestions.svelte";

  const MAX_RESULTS = 50;
  const RECENT = 8;

  type Row = {
    path: string;
    /** Character positions of the match inside the full path. */
    positions: number[];
    /** Character positions of the match inside the description (when that is why the file is listed). */
    descriptionPositions: number[];
    description: string;
    ms: number | null;
    group?: "changed" | "opened";
    /** A file the Explorer filter hides, listed because the hidden matches were revealed. */
    hidden?: boolean;
  };

  /** The words and the `tag:` filters of the box. */
  const q = new TagQueryState(() => tags.counts);
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let list: HTMLElement | undefined = $state();

  /** The "+N hidden files match" row was used: the files the Explorer filter hides are listed too. */
  let revealHidden = $state(false);

  /** The files the recent lists are drawn from. */
  const files = $derived(revealHidden ? vault.files : filter.files);

  const recency = $derived.by(() => {
    const map = new Map<string, number>();
    docs.recentPaths().forEach((p, i) => map.set(p, i === 0 ? 1 : 0));
    for (const e of feed.entries) if (e.type === "file") map.set(e.path, e.atMs);
    return map;
  });

  const describe = (path: string): string => tags.index.get(path)?.description ?? "";
  // chips of tags no file has, and a `tag:x` being typed that nothing matches
  const unknown = $derived([
    ...unknownTags(q.chips, tags.known),
    ...(q.partial !== null && q.partial !== "" && q.suggestions.length === 0 ? [q.partial] : []),
  ]);
  const suggesting = $derived(q.partial !== null && q.suggestions.length > 0);

  // tag-only queries list the files by recency: the time a file changed counts too
  const listRecency = $derived.by(() => {
    const map = new Map(recency);
    for (const entry of tags.index.values()) {
      map.set(entry.path, Math.max(map.get(entry.path) ?? 0, entry.modifiedMs));
    }
    return map;
  });

  /** The box holds a search (words or tags); otherwise it lists the recent files. */
  const searching = $derived(q.words !== "" || q.chips.length > 0);

  /** What the search finds. When only files the Explorer filter hides match, they are listed at once. */
  const matches = $derived.by<QuickMatches | null>(() => {
    if (!searching || unknown.length > 0) return null;
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
      limit: MAX_RESULTS,
    });
  });

  const rows = $derived.by<Row[]>(() => {
    if (!searching && unknown.length === 0) {
      const exists = new Set(files);
      const changed: Row[] = [];
      for (const e of feed.entries) {
        if (e.type !== "file" || !e.isDoc || e.kind === "removed" || !exists.has(e.path)) continue;
        changed.push({
          path: e.path,
          positions: [],
          descriptionPositions: [],
          description: describe(e.path),
          ms: e.atMs,
          group: "changed",
        });
        if (changed.length >= RECENT) break;
      }
      const seen = new Set(changed.map((r) => r.path));
      const opened = docs
        .recentPaths()
        .filter((p) => exists.has(p) && !seen.has(p))
        .slice(0, RECENT)
        .map<Row>((path) => ({
          path,
          positions: [],
          descriptionPositions: [],
          description: describe(path),
          ms: null,
          group: "opened",
        }));
      return [...changed, ...opened];
    }
    return (matches?.found ?? []).map((r) => ({
      ...r,
      description: describe(r.path),
      ms: feed.entries.find((e) => e.type === "file" && e.path === r.path)?.atMs ?? null,
    }));
  });

  // the files the Explorer filter hides that the query would also find, and are not listed
  const hiddenCount = $derived(matches?.moreHidden ?? 0);
  /** Rows plus the "hidden files match" row, which is selected like any other. */
  const rowCount = $derived(rows.length + (hiddenCount > 0 ? 1 : 0));
  const moreActive = $derived(onMoreRow(active, rows.length, hiddenCount));

  const emptyText = $derived.by(() => {
    if (unknown.length > 0) {
      return tags.counts.length === 0
        ? t("tags.noTagsAtAll", { name: unknown[0] })
        : t("tags.unknownTag", { name: unknown[0], tags: knownTagsHint(tags.counts) });
    }
    if (q.words === "" && q.chips.length > 0) return t("quick.tagsOnly");
    return q.isEmpty ? t("quick.empty") : t("quick.none", { query: q.words || q.text.trim() });
  });

  function close(): void {
    ui.quickOpen = false;
  }

  function reveal(): void {
    revealHidden = true;
    active = 0;
    input?.focus();
  }

  function choose(row: Row | undefined): void {
    if (!row) return;
    close();
    void (row.ms !== null && row.group === "changed"
      ? app.openChanged(row.path)
      : app.openDoc(row.path));
  }

  function onkeydown(e: KeyboardEvent): void {
    if (q.handleKey(e)) {
      e.preventDefault();
      return;
    }
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (rowCount === 0) return;
      active = (active + (e.key === "ArrowDown" ? 1 : -1) + rowCount) % rowCount;
      void tick().then(() =>
        list?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" }),
      );
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (moreActive) reveal();
      else choose(rows[active]);
    }
  }

  $effect(() => {
    void q.words;
    void q.chips;
    active = 0;
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

  function nameSegments(row: Row) {
    const start = row.path.lastIndexOf("/") + 1;
    return highlightSegments(
      basename(row.path),
      row.positions.filter((p) => p >= start).map((p) => p - start),
    );
  }

  function descriptionSegments(row: Row) {
    return highlightSegments(row.description, row.descriptionPositions);
  }

  function folderSegments(row: Row) {
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

  const groupTitle = { changed: "quick.recentChanged", opened: "quick.recentOpened" } as const;
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="overlay-top"
  onclick={(e) => {
    if (e.target === e.currentTarget) close();
  }}
>
  <div class="palette" role="dialog" aria-modal="true" aria-label={t("quick.label")}>
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
        oninput={(e) => q.setText(e.currentTarget.value)}
        {onkeydown}
        placeholder={q.chips.length > 0 ? "" : t("quick.placeholder")}
        aria-label={t("quick.label")}
        role="combobox"
        aria-expanded="true"
        aria-controls={suggesting ? "quick-tags" : "quick-list"}
        aria-activedescendant={suggesting
          ? `quick-tags-${q.active}`
          : rows[active] || moreActive
            ? `quick-row-${active}`
            : undefined}
        spellcheck="false"
        autocomplete="off"
      />
    </div>
    {#if suggesting}
      <div class="palette__list">
        <TagSuggestions
          id="quick-tags"
          rows={q.suggestions.map((match) => ({ type: "tag" as const, match }))}
          active={q.active}
          onpick={(row) => row.type === "tag" && q.pick(row.match.tag.name)}
          onhover={(i) => (q.active = i)}
        />
      </div>
    {:else}
      <div bind:this={list} id="quick-list" class="palette__list" role="listbox">
        {#each rows as row, i (row.group + row.path)}
          {#if row.group && (i === 0 || rows[i - 1].group !== row.group)}
            <div class="palette__group">{t(groupTitle[row.group])}</div>
          {/if}
          {@const badge = badgeFor(vault.git?.files[row.path]?.state)}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            id="quick-row-{i}"
            class={["palette__row", i === active && "is-active", row.hidden && "is-hidden"]}
            role="option"
            tabindex="-1"
            aria-selected={i === active}
            data-i={i}
            onmousemove={() => (active = i)}
            onclick={() => choose(row)}
          >
            <Icon name={isMarkdown(row.path) ? "file-text" : "file"} size="sm" />
            <div class="palette__body">
              <div class="palette__line">
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
          </div>
        {:else}
          <div class="palette__empty">{emptyText}</div>
        {/each}
        {#if hiddenCount > 0}
          <!-- svelte-ignore a11y_click_events_have_key_events -->
          <div
            id="quick-row-{rows.length}"
            class={["palette__row", "palette__row--more", moreActive && "is-active"]}
            role="option"
            tabindex="-1"
            aria-selected={moreActive}
            data-i={rows.length}
            onmousemove={() => (active = rows.length)}
            onclick={reveal}
          >
            <Icon name="eye" size="sm" />
            <span>{tn("filter.hiddenMatches", hiddenCount)}</span>
          </div>
        {/if}
      </div>
    {/if}
    <div class="palette__foot">
      <span>{t("quick.footMove")}</span><span>{t("quick.footOpen")}</span><span
        >{t("quick.footClose")}</span
      >
    </div>
  </div>
</div>
