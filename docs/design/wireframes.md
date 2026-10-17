# PerMark MVP: wireframes, flows, keyboard, components

Scope: the **MVP** only. Visual reference: `mockup.html` (IDE mode, light/dark).
Tokens and type come from the design system. Lucide icons only.

Legend: `[@]` copy reference, `[E]` open in external editor (**the slot "Edit" takes in v0.2**), `[:]` more menu,
`M A U D R C` git badges, `o` folder roll-up dot, `++` changed block (glow, then gutter bar), `--` removed-block marker,
`(o)` live dot, `[x]`/`[ ]` toggles, `[...]` buttons. All text in the UI is English and tool-agnostic, via `t()`.

## 1. Welcome

```
+------------------------------------------------------------------------------+
|                                                                              |
|                                  PerMark                                   |
|                 Read, navigate and follow your Markdown docs.                |
|                                                                              |
|        [ Open folder...          Ctrl+O ]   [ Open file...    Ctrl+Shift+O ] |
|                                                                              |
|        RECENT FOLDERS                                                        |
|      > permark-docs     F:\AIProjects\Markdown              2 hours ago    |
|        big-repo           F:\work\big-repo                      yesterday    |
|        notes              D:\old\notes  (not found)                 [Remove]    |
|                                                                              |
|        Drop a folder or a .md file anywhere to open it.                      |
|                                                                              |
+------------------------------------------------------------------------------+
```
- Recent list: Up/Down + Enter opens; `Del` removes. Missing folders are dimmed with "(not found)" and only offer Remove.
- **Drag over**: the window gets a 2px dashed accent outline and one centered line, "Drop to open. Folder: IDE mode.
  File: Simple mode." Several items dropped: "Drop one folder or one file."
- Empty recents: the list is replaced by "No recent folders yet." Loading after a pick: spinner in the button (no screen swap
  until `openVault` resolves; error shows inline under the buttons, e.g. "Can't open F:\x: access denied").

## 2. IDE mode (main)

```
+---+------------------------+--------------------------------------------------+--------------------------+
|[F]| EXPLORER           [:] | <- ->  docs / specs / live-feed.md  M  [@][E][:] | LIVE CHANGES    o active |
|   | v permark-docs       |--------------------------------------------------| [x] Docs only            |
|[S]|   v docs             o |    +----------------------------------------+    |--------------------------|
|   |     v specs          o |    | status: draft   updated: 2026-10-13    |    | M live-feed.md      12s  |
|[L]|      [live-feed.md]  M |    | tags: feed, watcher                    |    |   docs/specs  3 saves    |
|   |       watcher.md       |    +----------------------------------------+    | + ui-notes.md       1m   |
|   |     v research       o |                                                  |   @wt feature-ui         |
|   |       storage.md     U |    Live changes feed                             | - old-notes.md      3m   |
|   |       watchers.md    M |    ==========================================    |   docs  removed          |
|   |     > design           |    The feed lists every file the watcher sees,   | > checkout: 143 files 8m |
|   |   README.md            |    keyed by path and updated in place.           |                          |
|   |   plan.md            M |                                                  | v SINCE LAST VISIT  (5)  |
|   |                        |  ++Bursts of saves collapse into one entry.      |   since Mon 18:40        |
|   | WORKTREES              |  ++and never produce more than one toast.        |   docs/specs/            |
|   | > @wt feature-ui       |                                                  |     M live-feed.md       |
|   |   feature/ui           |  -- - - - 1 block removed - - - - - - [show]     |     A watcher.md         |
|   | > @wt fix-links        |                                                  |   docs/research/         |
|   |   fix/links            |    Event model                                   |     M watchers.md        |
|   |                        |    +------------+------------+--------------+    |   (root)                 |
|   |                        |    | Event      | Source     | Grouping     |    |     M plan.md            |
|   |                        |    | modified   | watcher    | by path      |    |                          |
|   |                        |    | checkout   | git        | bulk         |    |                          |
|   |                        |    +------------+------------+--------------+    |                          |
|[T]|                        |    [!] Note: temp files never reach the feed.    |                          |
|[*]|                        |    ...                                           |                          |
+---+------------------------+--------------------------------------------------+--------------------------+
| git: main   5 changed   Since: last visit v              State: Local   Follow [ ]   100%   Normal width |
+----------------------------------------------------------------------------------------------------------+
```
**Activity bar**: `[F]` Explorer and `[S]` Search are two views of the sidebar (clicking the active one collapses it),
`[L]` toggles the right panel (shows an accent dot when events arrive while it is hidden), `[T]` theme cycle (system/light/dark), `[*]` settings popover (theme, reading width,
reference template with a live preview). Panels collapse to give the doc the full width; state is saved in the session.

**Tree**: dirs first; badge letter in git color, file name tinted only for `D` (struck) and `C`. Folders show a roll-up dot
(worst state wins: conflicted > deleted > modified > added). Click opens; Enter/Space on keyboard; Left/Right collapse/expand.
The open doc is highlighted and revealed. Worktrees are extra top-level roots with their branch as a muted subline
(no badges inside worktrees in the MVP). Context menu: Copy path, Copy reference, Open in external editor.
`tree-changed` re-fetches the tree, keeping expansion and scroll.

**Doc header**: back/forward (disabled at the ends), breadcrumb (click a segment = reveal that folder in the tree),
git badge, "saved 12s ago" from `modifiedMs`. `[@]` copies `@{path}` (template), `[E]` opens externally, `[:]` holds
Copy path, Reveal in tree, Reading width, Zoom. `encodingNote` shows as a muted chip ("Decoded from UTF-16").

**Live changes** (right panel): one row per path (`fs-change` updates in place and moves it to the top), kind dot
(+ created, M modified, - removed, R renamed "from x"), folder or `@wt <name>` label, save count, relative time
(refreshed every 30s). Click opens the doc (removed: opens the tombstone). The open doc's row has an accent bar.
Header status: "(o) active" while events arrived in the last 30s, else "idle 4m"; nothing pulses with reduced motion.
"Docs only" (default on) hides non-Markdown files; the count of hidden ones shows as "+9 other files".
**Since last visit**: grouped by folder, collapsible, click opens. Header names the baseline time.

**Status bar**: branch (or `detached a1b2c3d`, `no commits yet`), change count, baseline menu ("Since: last visit" /
"Since: last commit", each item shows its time or short oid), Follow toggle, zoom (click resets), width toggle.
**Repo-state banner** (merge/rebase/cherry-pick/revert/bisect) sits under the doc header:
"Rebase in progress. Files may change quickly; large batches are grouped in the feed." with Dismiss (until state changes).
The status bar keeps a small "REBASING" chip after dismissal.

**On file change**: open doc changed on disk → re-render, scroll anchored to the first visible block, LCS diff marks
blocks (§7). Another doc changed → feed row + toast (§8). Tree badges update from `git-status-changed` without flicker.

### Project state: Local (default) or `.permark/` (opt-in)
By default nothing is written to the project. The Explorer `[:]` menu (Collapse all, Reveal open doc, **Initialize
PerMark in this project...**) and the status bar item `State: Local` (click opens a menu with the same command) start it.
A one-time hint sits at the foot of Since last visit: "Keep settings with the project? Initialize .permark/ [x]";
it never returns once dismissed or used (stored globally). After init the item reads `State: .permark`.

```
+------------------------------------------------------------+
| Initialize PerMark in this project?                      |
|                                                            |
| PerMark will create .permark/ in permark-docs:       |
|   config.toml   shared settings, safe to commit            |
|   cache/        search index (gitignored)                  |
|   local/        your session and last visit (gitignored)   |
|                                                            |
| Your current state moves there. Delete the folder to undo. |
|                                                            |
|                                     [Cancel]  [Initialize] |
+------------------------------------------------------------+
```
Modal, focus on Initialize, Esc cancels. Also writes `.permark/.gitignore` (`cache/`, `local/`). On failure (read-only
folder): inline error in the dialog, state stays Local. No undo command in the MVP; deleting the folder falls back to Local.

## 3. Simple mode (a single file was opened)

```
+------------------------------------------------------------------------------+
| <- ->  proposal.md   Downloads                       [@] [E] [:]   100%  [*] |
+------------------------------------------------------------------------------+
|                                                                              |
|                Proposal: shared glossary                                     |
|                =========================================                     |
|                We propose one glossary per repository, linked from           |
|                every README. See [the template].                             |
|                                                                              |
|                [!] Tip: images next to this file load as usual.              |
|                                                                              |
+------------------------------------------------------------------------------+
```
No sidebar, right panel or status bar. Live reload and block glow still work for this file (no toast, no Follow).
Links to sibling files navigate in place with back/forward. The `[:]` menu adds "Open containing folder" (IDE mode).

## 4. Find bar (Ctrl+F)

```
+------------------------------------------------------------------------------+
| <- ->  docs / specs / live-feed.md  M                        [@] [E] [:]     |
|                                   +----------------------------------------+ |
|                                   | watcher_          3 of 12  [^] [v] [x] | |
|                                   +----------------------------------------+ |
|    Live changes feed                                                         |
|    The {watcher} reconciles every batch with the disk. A [watcher] restart   |
|    triggers a full rescan.                                                   |
+------------------------------------------------------------------------------+
```
Floats top-right of the doc, does not shift content. Case-insensitive, accent-insensitive, as-you-type (debounced 100ms).
All matches `{...}` use the `--mark` token; the current one `[...]` gets an accent outline and is scrolled to center.
Enter / Shift+Enter next/prev, Esc closes and restores focus. "No results" turns the counter muted red. A live
reload re-runs the search and keeps the current index when possible.

## 5. Quick open (Ctrl+P)

```
+------------------------------------------------------------------------------+
|            +----------------------------------------------------+            |
|            | > _                                                |            |
|            +----------------------------------------------------+            |
|            | RECENTLY CHANGED                                   |            |
|            | [live-feed.md]     docs/specs            12s  M    |            |
|            |  ui-notes.md       @wt feature-ui         1m       |            |
|            |  watchers.md       docs/research          4m  M    |            |
|            | RECENTLY OPENED                                    |            |
|            |  plan.md           (root)                     M    |            |
|            |  README.md         (root)                          |            |
|            +----------------------------------------------------+            |
|            | Up/Down move   Enter open   Esc close              |            |
|            +----------------------------------------------------+            |
+------------------------------------------------------------------------------+
```
Empty query: feed paths, then in-session history. Typing: fuzzy over all tree paths; matched letters bold; rank name
prefix > name substring > path, ties by recency. Max 50 rows, virtualized not needed. "No files match 'xyz'" when empty.

## 6. Search panel (Ctrl+Shift+F), section-level full text

```
+--------------------------------------+   +--------------------------------------+
| SEARCH                               |   | SEARCH                               |
| +--------------------------------+   |   | +--------------------------------+   |
| | character descriptions_      x |   |   | | quarterly budget_            x |   |
| +--------------------------------+   |   | +--------------------------------+   |
| [ ] Semantic (v0.2)    12 in 3 files |   | [ ] Semantic (v0.2)                  |
|--------------------------------------|   | Indexing 412 of 1,280 files          |
| v lore.md (2)             docs/world |   | [=========>                    ]     |
|  [Characters > Aria]                 |   |--------------------------------------|
|   {Character} notes: tall, quiet,    |   | No results yet. The index is still   |
|   {descriptions} change per book...  |   | being built; results may be partial. |
|   Characters > Bram                  |   |                                      |
|   ...the {character} sheet lists...  |   | Done and still nothing:              |
| > outline.md (4)          docs/world |   | No results for "quarterly budget".   |
| > glossary.md (6)    docs            |   | Ctrl+P searches file names instead.  |
+--------------------------------------+   +--------------------------------------+
```
- A sidebar view (activity bar `[S]`), so it shares the 260px column; the sidebar may be dragged wider for long snippets.
- Query runs 150ms after typing stops; Enter runs at once. Header shows "12 in 3 files"; groups ordered by best score,
  hits by score. Each hit: heading breadcrumb (`Characters > Aria`, file name in the group header; for a hit before
  the first heading, the file title) and a 2-line snippet, ellipsized, `<mark>` terms in the `--mark` token.
- **Open a hit** (click or Enter): open the doc, scroll the heading at `startLine` to the top third, give the section a
  1.5s `--mark` tint that fades (accent stays reserved for live changes), and open the find bar pre-filled with the query
  so Enter steps through the terms. Focus stays in the results so Down + Enter walks hit by hit.
- Keyboard: Down from the query enters the list; Up/Down over hits; Left/Right collapse/expand a file group;
  Enter opens; Esc clears the query, a second Esc returns focus to the doc. Ctrl+Shift+F selects the query text.
- **Indexing** (`index-progress`): thin progress bar under the query with "Indexing 412 of 1,280 files"; search works on
  what is indexed and says "results may be partial". Finished: the line disappears. Live saves re-index silently.
- **Empty query**: "Search the text of every doc. Results point to the section." **No results**: as above, with the
  Ctrl+P hint. **Error**: "Search index unavailable: <reason>" + Retry.
- `Semantic (v0.2)` is a visible but disabled toggle; tooltip "Meaning-based search arrives in v0.2 (optional local model)".

## 7. Doc right after a live update

```
+------------------------------------------------------------------------------+
| <- ->  docs / specs / live-feed.md  M   saved just now          [@] [E] [:]  |
+------------------------------------------------------------------------------+
|    Grouping                                                                  |
|                                                                              |
| ++ Bursts of saves collapse into one entry and never produce more than one   |
| ++ toast; the toast updates its counter in place.                            |
|                                                                              |
| -- - - - - - - 1 block removed  10:42 - - - - - - - - - - - - - - [Show]     |
|                                                                              |
|    Rollout                                                                   |
| ++ [x] Aggregate events (150 ms quiet, 1 s max)                              |
| ++ [x] Reconcile with disk and drop duplicates                               |
| ++ [ ] Follow dwell: at most one jump every 3 s                              |
+------------------------------------------------------------------------------+
After [Show]:
| -- - - - - - - 1 block removed  10:42 - - - - - - - - - - - - - - [Hide]     |
| --   Each save shows its own toast, so nothing is missed.                    |
```
- Changed/added block: background accent 18% that fades over 2.5s, then a 3px accent gutter bar at 50% stays until the
  next update of this doc or navigation away (so a glance back still shows what moved). Reduced motion: no fade, bar only.
- Removed: a dashed line in the deleted color with "N blocks removed · time". Show reveals the old HTML (kept from the
  previous render, no extra API) dimmed with a red left bar. Markers clear on the next update or navigation.
- Granularity is the **top-level block**: ticking one checkbox marks the whole list (per-item: v0.2).
- Scroll is anchored to the first visible unchanged block; the view never jumps unless Follow is on.

## 8. Bulk event in the feed + burst toast

```
+--------------------------------------------------+---------------------------+
|                                                  | LIVE CHANGES     o active |
|    ...document...                                | [x] Docs only             |
|                                                  |---------------------------|
|                                                  | M watchers.md    just now |
|                                                  | + storage.md     just now |
|                                                  | M index.md             2s |
|                                                  | M glossary.md          3s |
|                                                  | v checkout: 143 files 8m  |
|                                                  |     git operation, 12 docs|
| +----------------------------------------------+ |     M spec.md   docs      |
| | (*) 4 docs changed - latest: watchers.md     | |     M plan.md   (root)    |
| |     docs/research, just now   [Open]  [x]    | |     Show all 143 files    |
| +----------------------------------------------+ |                           |
+--------------------------------------------------+---------------------------+
```
- **Bulk** (`FsBatch.bulk` set: git operation, large batch, overflow): one collapsed row, **no toast, no Follow**; expanding
  lists what `changes` carried (first 50, then "Show all"); `rescan` adds "Feed resynced" to the row.
- **Burst** (normal events): **one toast at a time**. While new docs change within 10s, the toast updates in place
  ("4 docs changed - latest: x") instead of stacking. Auto-hides after 6s, hover pauses, Open jumps to the latest.
  No toast for the open doc (it glows instead) or for non-doc files when "Docs only" is on.

## 9. Follow active

```
+------------------------------------------------------------------------------+
| <- ->  docs / research / watchers.md  M   saved 1s ago          [@] [E] [:]  |
| (o) Following: jumped from live-feed.md             [Back Alt+Left]  [Stop]  |
+------------------------------------------------------------------------------+
|    Watchers                                                                  |
| ++ Debounce 150 ms, then reconcile with the disk.                            |
|                                                                              |
+------------------------------------------------------------------------------+
| git: main   6 changed   Since: last visit v           [(o) Following]   100% |
+------------------------------------------------------------------------------+
```
Follow on: a changed doc (Markdown only) opens automatically, at most once per 3s, never for bulk; the first changed block
scrolls into view. Skipped (toast instead) while find, quick open or a popover is open, or text is selected.
The jump chip fades after 5s. In-doc: when the open doc changes, scroll to its first changed block.

## 10. Selection, "copy with reference"

```
+------------------------------------------------------------------------------+
|                  +-------------------------------------------+               |
|                  | Copy with reference  Ctrl+Alt+C  |  Copy  |               |
|                  +-------------------------------------------+               |
|    The watcher reconciles every batch with the disk before                   |
|    [emitting it, so a save that was undone never reaches the feed.]          |
+------------------------------------------------------------------------------+
```
Appears 150ms after a mouse selection ends (not for keyboard selection: use the shortcut), above the selection,
flips below near the top edge; Esc or click elsewhere closes. Clipboard (reference from the template, section =
nearest heading above the selection start):
```
@docs/specs/live-feed.md (section "Event model")
> emitting it, so a save that was undone never reaches the feed.
```
Feedback: the button reads "Copied" for 1.2s. Same check-mark feedback for `[@]` and Copy path.

## 11. Empty and error states

**Binary / too large** (doc area, header still present):
```
+------------------------------------------------------------------------------+
| <- ->  docs / report.pdf                                        [@] [E] [:]  |
+------------------------------------------------------------------------------+
|                        This file can't be shown here.                        |
|                report.pdf looks like a binary file (1.2 MB).                 |
|                   [Open in external editor]   [Copy path]                    |
+------------------------------------------------------------------------------+
```
Too large: "big-api.md is too large to render (12.4 MB; the limit is 5 MB)." with the same buttons.

**Broken link** (on click, `resolveLink` returns `missing`):
```
+------------------------------------------------------------------------------+
|    See the [old spec] for background.                                        |
|            ~~~~~~~~~~                                                        |
|          +-------------------------------------------------+                 |
|          | Not found: docs/old/spec.md         [Copy path] |                 |
|          +-------------------------------------------------+                 |
+------------------------------------------------------------------------------+
```
The link gets a wavy deleted-color underline for the rest of the visit; no navigation, history unchanged.
Missing anchor in an existing doc: open the doc at the top and say "Section #x not found" in a toast.

**File deleted while open** (tombstone, only after the watcher's reconcile confirms it is gone):
```
+------------------------------------------------------------------------------+
| <- ->  docs / specs / watcher.md  D                           [@] [E] [:]    |
+------------------------------------------------------------------------------+
| (!) This file was deleted at 10:42. You are reading the last version.        |
|     Moved to docs/specs/watchers.md   [Open]   [Close]                       |
+------------------------------------------------------------------------------+
|    (last rendered content, dimmed to 60%, still selectable and searchable)   |
+------------------------------------------------------------------------------+
```
"Moved to" appears only for a `renamed` change. If the file reappears (delete + create save), the banner clears and the
diff runs against the last version.

**Git error** (`gitError` set; banner on top of Since last visit, status bar item in the modified color):
```
+------------------------------------------------------------------------------+
| (!) Git can't read this repository: dubious ownership.                       |
|     Change markers are off; Since last visit uses file times.                |
|     git config --global --add safe.directory F:/work/big-repo   [Copy]       |
+------------------------------------------------------------------------------+
| (!) Git unavailable   Since: file times                    Follow [ ]   100% |
+------------------------------------------------------------------------------+
```
Shows `gitError` verbatim; the fix line only for recognized errors. Never shows "no changes" when git failed.

**Not a git repo**: calm, not an error. Status bar shows "No git repository"; no badges, no baseline menu;
Since last visit header reads "By file times, since Mon 18:40".

**Other states**
- Loading a doc: keep the previous doc, show a 2px indeterminate bar under the header after 150ms; skeleton only on first load.
- Empty folder: "No Markdown files here yet. New files appear as soon as they are written."
- No doc open in IDE mode: open `README.md`/`index.md` if present, else "Pick a file in the tree or press Ctrl+P."
- Empty feed: "Nothing has changed since you opened this folder. Saves from any editor or tool show up here."
- Empty Since last visit: "Nothing changed since Mon 18:40." First visit: "First visit: changes are tracked from now."
- Truncated git status (`truncated`): status bar count reads "5,000+ changed".
- Mermaid or math error: the block shows the source in a code block with a muted "Diagram could not be rendered" line.

## 12. Flows

**Open project**
1. Launch (or `permark <path>`); a folder argument skips Welcome.
2. Welcome: pick a recent (Enter) or Open folder (Ctrl+O), or drop a folder.
3. Tree, git snapshot and Since last visit load in parallel; the last doc and scroll are restored from the session.

**Morning catch-up**
1. Open the project; Since last visit is expanded on the right, with its baseline time.
2. Click the first file; it opens (changes vs baseline inside the page are v0.2).
3. Optionally switch the status bar to "Since: last commit" to see only uncommitted work.

**Follow an agent live**
1. Start the agent in a terminal; keep PerMark next to it.
2. Toggle Follow (Ctrl+Shift+L or the status bar).
3. Each save of a doc opens it and scrolls to the changed block; glow fades, gutter bars remain.
4. Read freely: selecting text or opening find pauses jumps; the toast still reports changes.
5. Feed shows "idle 2m" when the agent stops; F7 / Shift+F7 walks the changed blocks.

**Burst of 15 files**
1. Agent saves 6 docs + 9 other files in 40s.
2. Feed (Docs only) shows 6 rows, "+9 other files"; one toast counts up "6 docs changed".
3. Follow, if on, dwells: one jump per 3s, docs only. A checkout during this is one bulk row, no jump.
4. Click rows in order (or Ctrl+P, empty query) to review each doc.

**Find information** ("where are the character descriptions?")
1. Ctrl+Shift+F, type `character descriptions`; results group by file with section breadcrumbs.
2. Down to the best hit (`lore.md > Characters > Aria`), Enter: the doc opens at that section, tinted briefly,
   terms highlighted and the find bar ready.
3. Down + Enter for the next hit, or Enter in the find bar to step through terms in this doc.
4. Knows the file name instead: Ctrl+P. Follow links; Alt+Left to return.

**Send feedback to the AI**
1. Select the passage; the popover appears.
2. Copy with reference (or Ctrl+Alt+C); paste in the agent's terminal and type the request.
3. Without a selection, `[@]` or Ctrl+Alt+C copies just the reference `@docs/x.md`.

## 13. Keyboard

| Action | Shortcut | Notes |
|---|---|---|
| Open folder / open file | Ctrl+O / Ctrl+Shift+O | Replaces the current project (session is saved) |
| Search everywhere (files, sections, tags, commands) | Ctrl+T | Tab / Shift+Tab switch the scope; `>` is Commands, `#` is Sections (v0.3 section 1) |
| Quick open (file names) | Ctrl+P | Search everywhere in the Files scope; empty query lists recent changes |
| Commands | Ctrl+Shift+P | Search everywhere in the Commands scope |
| Search all docs (sections) | Ctrl+Shift+F | Opens the Search view, selects the query |
| Explorer view | Ctrl+Shift+X | Focuses the tree |
| Find in page | Ctrl+F, Enter, Shift+Enter, Esc | |
| Back / forward | Alt+Left / Alt+Right, mouse 4/5 | |
| Next / previous changed block | F7 / Shift+F7 | Includes removed markers |
| Next unread document | Alt+U | The live feed first, newest first, then the tree (v0.3 section 2) |
| Toggle Follow | Ctrl+Shift+L | |
| Copy reference (or selection with reference) | Ctrl+Alt+C | Context-aware |
| Copy path | Shift+Alt+C | VS Code parity |
| Open in external editor | Ctrl+Shift+E | Becomes Edit (Ctrl+E) in v0.2 |
| Toggle sidebar / right panel | Ctrl+B / Ctrl+Alt+B | |
| Zoom in / out / reset | Ctrl+= / Ctrl+- / Ctrl+0 | Also Ctrl+wheel |
| Reading width | Ctrl+Alt+W | Cycles Comfortable 760px / Wide 1100px / Full (v0.2 section 15) |
| Settings | Ctrl+, | Popover |
| Close any overlay | Esc | Focus returns to where it was |
| Reserved for v0.2 | Ctrl+K palette, Ctrl+E edit, Ctrl+S save | Do not bind in MVP |

Tree: arrows, Enter, Home/End, type-ahead. Feed and lists: Up/Down, Enter. Every action is reachable by Tab with a visible
2px accent focus ring.

## 14. Component inventory (12 components)

| Component | States | Used in |
|---|---|---|
| IconButton | default, hover, pressed, toggled-on, focus, disabled, "done" check (1.2s) | everywhere |
| TreeItem | file/dir/worktree root; hover, selected (open doc), focus, collapsed/expanded, badge M/A/U/D/R/C, roll-up dot, live-updated (badge fades in 180ms) | sidebar |
| DocHeader | nav enabled/disabled, badge, saved-time, encoding chip, loading bar, follow chip | IDE, Simple |
| DocView | rendering, rendered, changed (glow -> gutter), removed marker (closed/open), tombstone (dimmed), find marks | main |
| Notice | empty state (icon, title, text, actions); banner info/warning/error, dismissible | doc, panel |
| SectionHeader | expanded/collapsed, count, action slot | right panel, tree |
| FeedRow | kind created/modified/removed/renamed, current doc, worktree label, saves count, hover, focus; bulk collapsed/expanded | right panel, quick open |
| StatusItem | text, menu, toggle on/off, warning | status bar |
| Toast | single, coalesced (counter), hover-paused, leaving | main |
| Popover / Menu | baseline menu, more menu, settings, selection popover, missing-link popover; item hover/checked/disabled | overlays |
| Overlay input + list | quick open, find bar; empty, results, active row, no results | overlays |
| SearchHit (+ file group) | group collapsed/expanded with count; hit hover, active (keyboard), focus; snippet marks; panel states empty/indexing/no results/error; disabled Semantic toggle | sidebar Search view |
| Dialog | confirm (Initialize .permark), inline error, busy | modal |

Frontmatter card and alerts are pure CSS in `markdown.css`, not components.

## 15. Flags: spec conflicts and MVP cuts

- **Section search moved into the MVP** (was v0.2 in `plan.md`; it also adds `search`/`index-progress` to the contract).
  It is the largest UI addition here (~20 min UI on top of the Rust index). Simpler variant if time runs out: flat hit
  list (no file grouping), open at the heading without the find-bar pre-fill.
- **`.permark/` reverses decision N11** ("state never in the repo") for users who opt in. Kept safe by: opt-in only,
  `cache/` and `local/` gitignored. Agents may read `config.toml`; keep it small and documented.
- **No command palette** in the MVP, so "Initialize PerMark" lives in the Explorer menu and the `State:` status item;
  it joins Ctrl+K in v0.2.

- **No tabs** in the MVP (design system layout says "tabs + doc"): one doc plus history. Tabs: v0.2.
- **Right panel has no Outline** in the MVP (design system says "outline / live changes"); the header reserves a
  segmented switch "Changes | Outline" for v0.2.
- **Ctrl+K command palette** is in the design system principles but not in MVP scope: reserved, unbound.
- **Git colors in light theme**: `#d29922` and `#3fb950` fail AA as text on white. Badge letters use darker variants
  in light theme (`#9a6700`, `#1a7f37`, `#cf222e`, `#8250df`); dots and dark theme keep the spec values.
- **Find highlight** uses a neutral `--mark` token, because the accent means "live change" and must not be overloaded.
- **Contract gaps**: `ChangedSince.files` has no time, so Since last visit is grouped by folder, sorted by name;
  adding `mtimeMs` enables newest-first (small Rust change, recommended). For bulk, the feed needs `changes` filled
  (capped) to list files; otherwise the expanded row only shows the count.
- Additions beyond the spec, all small: gutter bar after the glow, F7 next change, "idle" indicator, Docs-only toggle,
  coalesced toast, Follow skipped during selection/find, auto-open README. Cut any of them first if time runs short.
- Too big for the MVP, deferred: per-list-item diff, image viewer for binary images, "Show as plain text" for too-large
  files, opening another project in a new process from inside the app, styling broken links before they are clicked.

## Addendum (user validation, 2026-10-13): collapsible side panels
- Left panel (Explorer/Search) and right panel (Live changes / Since last visit) can each be **collapsed and expanded**:
  Ctrl+B (left), Ctrl+Alt+B (right), a collapse chevron in each panel header, and clicking the active activity-bar icon toggles the left panel.
- Both are resizable by dragging their inner edge (min 200 px, max 480 px); double-click the edge resets the width.
- Open/closed state and widths persist per project (SessionState). With both collapsed, the doc gets the full width (focus reading);
  the status bar shows small toggle buttons so a collapsed panel is always recoverable.
- Collapse/expand animates 150 ms (instant with prefers-reduced-motion).
