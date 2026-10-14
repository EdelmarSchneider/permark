<script lang="ts">
  import { onMount, tick } from "svelte";
  import { rankPaths } from "../fuzzy";
  import { gitStateLabel } from "../gitLabels";
  import { t } from "../i18n";
  import { basename, dirname, isMarkdown, splitWorktree } from "../paths";
  import { registerOverlay, rememberFocus } from "../overlays";
  import { highlightSegments } from "../text";
  import { app } from "../stores/app.svelte";
  import { clock } from "../stores/clock.svelte";
  import { docs } from "../stores/docs.svelte";
  import { feed } from "../stores/feed.svelte";
  import { ui } from "../stores/ui.svelte";
  import { vault } from "../stores/vault.svelte";
  import { relativeTime } from "../time";
  import { badgeFor } from "../tree";
  import Icon from "./Icon.svelte";

  const MAX_RESULTS = 50;
  const RECENT = 8;

  type Row = {
    path: string;
    /** Character positions of the match inside the full path. */
    positions: number[];
    ms: number | null;
    group?: "changed" | "opened";
  };

  let query = $state("");
  let active = $state(0);
  let input: HTMLInputElement | undefined = $state();
  let list: HTMLElement | undefined = $state();

  const files = $derived(vault.files);

  const recency = $derived.by(() => {
    const map = new Map<string, number>();
    docs.recentPaths().forEach((p, i) => map.set(p, i === 0 ? 1 : 0));
    for (const e of feed.entries) if (e.type === "file") map.set(e.path, e.atMs);
    return map;
  });

  const rows = $derived.by<Row[]>(() => {
    const q = query.trim();
    if (q === "") {
      const exists = new Set(files);
      const changed: Row[] = [];
      for (const e of feed.entries) {
        if (e.type !== "file" || !e.isDoc || e.kind === "removed" || !exists.has(e.path)) continue;
        changed.push({ path: e.path, positions: [], ms: e.atMs, group: "changed" });
        if (changed.length >= RECENT) break;
      }
      const seen = new Set(changed.map((r) => r.path));
      const opened = docs
        .recentPaths()
        .filter((p) => exists.has(p) && !seen.has(p))
        .slice(0, RECENT)
        .map<Row>((path) => ({ path, positions: [], ms: null, group: "opened" }));
      return [...changed, ...opened];
    }
    return rankPaths(q, files, recency, MAX_RESULTS).map((r) => ({
      path: r.path,
      positions: r.positions,
      ms: feed.entries.find((e) => e.type === "file" && e.path === r.path)?.atMs ?? null,
    }));
  });

  function close(): void {
    ui.quickOpen = false;
  }

  function choose(row: Row | undefined): void {
    if (!row) return;
    close();
    void (row.ms !== null && row.group === "changed"
      ? app.openChanged(row.path)
      : app.openDoc(row.path));
  }

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (rows.length === 0) return;
      active = (active + (e.key === "ArrowDown" ? 1 : -1) + rows.length) % rows.length;
      void tick().then(() =>
        list?.querySelector(`[data-i="${active}"]`)?.scrollIntoView({ block: "nearest" }),
      );
    } else if (e.key === "Enter") {
      e.preventDefault();
      choose(rows[active]);
    }
  }

  $effect(() => {
    void query;
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
      <input
        bind:this={input}
        bind:value={query}
        {onkeydown}
        placeholder={t("quick.placeholder")}
        aria-label={t("quick.label")}
        role="combobox"
        aria-expanded="true"
        aria-controls="quick-list"
        aria-activedescendant={rows[active] ? `quick-row-${active}` : undefined}
        spellcheck="false"
        autocomplete="off"
      />
    </div>
    <div bind:this={list} id="quick-list" class="palette__list" role="listbox">
      {#each rows as row, i (row.group + row.path)}
        {#if row.group && (i === 0 || rows[i - 1].group !== row.group)}
          <div class="palette__group">{t(groupTitle[row.group])}</div>
        {/if}
        {@const badge = badgeFor(vault.git?.files[row.path]?.state)}
        <!-- svelte-ignore a11y_click_events_have_key_events -->
        <div
          id="quick-row-{i}"
          class={["palette__row", i === active && "is-active"]}
          role="option"
          tabindex="-1"
          aria-selected={i === active}
          data-i={i}
          onmousemove={() => (active = i)}
          onclick={() => choose(row)}
        >
          <Icon name={isMarkdown(row.path) ? "file-text" : "file"} size="sm" />
          <span class="palette__name">
            {#each nameSegments(row) as seg, s (s)}{#if seg.match}<b>{seg.text}</b
                >{:else}{seg.text}{/if}{/each}
          </span>
          <span class="palette__path">
            {#each folderSegments(row) as seg, s (s)}{#if seg.match}<b>{seg.text}</b
                >{:else}{seg.text}{/if}{/each}
          </span>
          {#if row.ms !== null}<span class="palette__time">{relativeTime(row.ms, clock.now)}</span
            >{/if}
          {#if badge}
            <span class={["badge", badge.className]} title={gitStateLabel(badge.state)}
              >{badge.letter}</span
            >
          {/if}
        </div>
      {:else}
        <div class="palette__empty">
          {query.trim() === "" ? t("quick.empty") : t("quick.none", { query: query.trim() })}
        </div>
      {/each}
    </div>
    <div class="palette__foot">
      <span>{t("quick.footMove")}</span><span>{t("quick.footOpen")}</span><span
        >{t("quick.footClose")}</span
      >
    </div>
  </div>
</div>
