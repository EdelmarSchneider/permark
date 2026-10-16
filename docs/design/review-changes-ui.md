# Review changes: the UI, level 1 (always on, quiet)

Build step 2 of `review-changes.md`. It specifies the review bar and the review layer in the **Read** view, their keys,
undo, states, tokens and strings, plus the hooks level 2 (review mode per project) will plug into. The Source line-diff
toggle is out of scope.

Principle: a reader who does not care sees, at most, one slim bar on a document that changed, and nothing else.

## 1. Two states of one bar

The bar lives in the banner slot under the document header (same place as `Banners.svelte` and `EditBars.svelte`), so it
stays put while the document scrolls. It is a new component, `ReviewBar.svelte`, plus `reviewLayer.ts` (DOM decoration)
and a store, `stores/review.svelte.ts`.

| State | Shown when | Content |
|---|---|---|
| hidden | the document is not Markdown in a project, Simple mode, notes, editing, status other than `changes`, or the bar was dismissed | nothing |
| **offer** | Read view, `reviewGet(path).status === "changes"` | `Changed since you last read · 3 changes` `[Review]` `[×]` |
| **reviewing** | the person chose Review (button, `[:]` menu, command) | `Reviewing 3 changes` `[‹] 1 / 3 [›]` `[Keep all]` `[Revert all]` `[Done]` |
| done flash | the last change was kept or reverted | `All changes reviewed` for 2 s, then hidden |

- **When the store asks**: on every fresh render of a document, and after a live update, debounced 300 ms (one call per
  burst). Never while editing; once on leaving edit mode.
- **Offer bar style**: a new `banner--quiet` variant: `surface` background, `muted` text, no colored icon, 32 px tall,
  12px UI font. `[Review]` is a ghost button with accent text; `[×]` an `IconButton`.
- **Dismiss `[×]`**: hides the bar for this document until its file hash changes or it is opened again. It does **not**
  keep anything and does not release the held version.
- **Reviewing bar**: the normal `banner` look (still neutral, no tint). `[Done]` and `Esc` close the layer and return to
  the offer bar if changes remain.
- Count: `changes.length`. The position `1 / 3` is the *current* change (section 4).
- Entry points at level 1: the bar's `[Review]`, an entry **Review changes** in the header `[:]` menu (enabled when status
  is `changes`), and Search everywhere commands (section 8).

## 2. Wireframes

Normal width (main pane >= 560 px):

```
┌ docs / spec.md ─────────────────────────────────────────────────────────────── [✎] [@] [:] ┐
│ ⇄ Changed since you last read · 3 changes                                [Review]     [×]  │  offer
└────────────────────────────────────────────────────────────────────────────────────────────┘

┌ docs / spec.md ─────────────────────────────────────────────────────────────── [✎] [@] [:] ┐
│ ⇄ Reviewing 3 changes        [‹] 1 / 3 [›]            [Keep all] [Revert all]      [Done]  │  reviewing
├────────────────────────────────────────────────────────────────────────────────────────────┤
│              ## Goals                                                                      │
│                                                                                            │
│              Changed · 1 of 3                                     [✓ Keep] [↶ Revert]      │  label row
│           ┃  We ship the reader first and the editor later.  (struck, muted, red tint)     │  <del>
│           ┃  We ship the reader, the editor and review together.  (green tint)             │  <ins>
│                                                                                            │
│              Unchanged text renders as usual, with no marks.                               │
│                                                                                            │
│              Removed · 2 of 3                                                              │  buttons on hover
│           ┆  ▸ 2 blocks removed: "Non-goals: no sync, no plugins…"            Show         │  collapsed run
└────────────────────────────────────────────────────────────────────────────────────────────┘
```

Narrow (main pane < 560 px): the bar folds its secondary actions into `[⋯]`; the label row keeps icon-only buttons.

```
┌──────────────────────────────────────────────┐
│ ⇄ 3 changes                  [Review]   [×]  │  offer
└──────────────────────────────────────────────┘
┌──────────────────────────────────────────────┐
│ [‹] 1/3 [›]                    [⋯]  [Done]   │  [⋯] = Keep all, Revert all
├──────────────────────────────────────────────┤
│  Changed · 1 of 3                  [✓] [↶]   │
│┃ We ship the reader first and the            │  struck
│┃ editor later.                               │
│┃ We ship the reader, the editor and          │
│┃ review together.                            │
└──────────────────────────────────────────────┘
```

Below 400 px the offer bar drops "Changed since you last read ·" and keeps `3 changes`; `[Done]` becomes an `x` icon.

## 3. The layer in the document

The layer **decorates the document DocView already rendered** (so diagrams, code blocks, code references and images
look like themselves) and inserts the old blocks from `ReviewView`. It never re-renders the document itself.

**Anchoring.** Each change is placed by the lines of its new blocks in the file, matched to the rendered top-level
blocks' `data-sourcepos`. A rendered block belongs to a change when its lines overlap the change's new lines. A pure
removal is placed before the first rendered block that starts after its position (or at the end). See section 11 for
the two fields this needs from the core.

**Wrapper.** Each change becomes a `div.rv-change[data-kind][data-index]` with `role="group"`, `tabindex="-1"` and
`aria-label="Change 2 of 5, modified"`. Inside, in order: the label row, then its rows.

| Kind | Rendering |
|---|---|
| `modified` | For each row, in order: `old` as `<del class="rv-old">` (line-through, `muted` text, deleted tint, dashed red left bar), then `new` as `<ins class="rv-new">` (normal text, added tint, solid green left bar). A row with only `old` or only `new` renders that side alone, inline (not collapsed). |
| `added` | The rendered blocks wrapped in `<ins class="rv-new">`. |
| `removed` | A collapsed marker, the existing `.tm-removed` look: `▸ N blocks removed: "<first 60 chars of the first block's text>"` and `Show`. Expanding shows every old block as `rv-old`. Expanded state survives a refresh (keyed by `old.startByte`). |

- Old blocks: their `html` goes through the reading view's enhancement, but links and code references are inert
  (`pointer-events: none`, as `.tm-removed__content` does today) and images are shown at 50% opacity.
- Non-color cues: `<del>`/`<ins>` semantics, line-through, dashed versus solid bar, and the label row's words. Red and green
  are never the only signal.
- Empty `html` (could not be drawn): show `text` in a `pre` with the code font.

**Label row** (24 px, 12px UI font, `muted`): `Changed · 2 of 5` / `Added · 2 of 5` / `Removed · 2 of 5`, then on the
right `[✓ Keep] [↶ Revert]` (small ghost buttons, Lucide `check` and `undo-2`).

- The **current** change shows its buttons always, and a 1 px `accent-muted` ring around the wrapper (radius 6 px).
- Other changes keep the row and the words; their buttons are `opacity: 0` until the wrapper is hovered or has focus
  within (they stay in the tab order and become visible on focus). No layout shift.
- Clicking anywhere in a change makes it current (no scroll).

**Focus order** (Tab): bar (`‹`, `›`, `Keep all`, `Revert all`, `Done`), then the document in reading order; for each
change: `Show` (removals), `Keep`, `Revert`. N and P move focus to the change wrapper, so one Tab reaches its Keep.

## 4. Current change and navigation

- Opening the layer makes change 1 current, scrolls it to 20% from the top (as `jumpToChange` lands), focuses its wrapper.
- `‹`/`›`, N/P, F7/Shift+F7 move to the previous/next change and wrap around; smooth scroll (instant under
  `prefers-reduced-motion`).
- After **Keep** or **Revert**, the same index becomes current (that is the next change, since the acted one is gone),
  or the previous one if it was the last. When none remain: the done flash, the layer closes, focus goes to the document
  region.
- After a **refresh** (agent wrote), the current change is found again by its position in the *reviewed* text
  (`old.startByte`), which an agent's write cannot move: the change whose `old` range contains or is nearest after the old
  position becomes current. Scroll is not touched.

## 5. Live updates and the review layer

| Situation | Behavior |
|---|---|
| Layer closed (offer bar or no bar) | The live glow, the accent gutter and "N blocks removed" markers work exactly as today. The offer bar appears or updates its count after the debounced `reviewGet`. |
| Layer open, agent writes | The update renders with the live marks off (render request `quiet` for marks only, still news for the feed). The store refreshes the review; the layer redecorates; changes whose new blocks were in the renderer's `changed` list play the existing `tm-glow` once over their tint (no accent gutter). Count and `n / m` update. Position kept (section 4). |
| Layer open, the person presses a key while a refresh is pending | Keep/Revert wait for the refresh; nothing acts on a view older than what is on screen. |
| Keep/Revert answered `reviewStale` | Nothing was written. The review refreshes, the same change (by old position) is current, and a warn toast says `review.stale`. The action is **not** repeated. |
| The file is deleted while reviewing | The layer closes; the tombstone banner takes over (section 6). |

Announcements for live refreshes are rate-limited: at most one `review.sr.updated` every 5 s.

## 6. States

| Status from `reviewGet` | Level 1 behavior |
|---|---|
| `changes` | Offer bar; layer on demand. |
| `unchanged` | Nothing. |
| `noBaseline` | No bar (the unread dot already said it changed). The `[:]` entry and the command are disabled with the tooltip `review.noBaseline`. |
| `tooLarge` | No bar. Disabled entry, tooltip `review.tooLarge`. **Keep all** is offered in Search everywhere only. |
| `notText` | No bar, no entry. |
| `new` | No bar at level 1 (everything is new; there is nothing to compare). The layer can render it (one `added` change) for level 2. |
| `deleted` (document open when it disappears) | The existing tombstone banner gains **Restore** when `restorable`: `reviewRestoreDeleted`; on success a toast `review.restored`, the document reopens normally. Close keeps today's meaning (and accepts the deletion: no core call needed at level 1). |
| request fails | No bar; errors are not shown for the automatic check. When the person asked (button, command): toast with the error text. |

## 7. Keys

Active only while the **layer is open**, the Read view has the document, no overlay is open and focus is not in a text
field (same rule as `yieldsToTyping` for the plain-letter keys).

| Key | Action | Notes |
|---|---|---|
| N / P | Next / previous change | Plain N and P are unbound today (only C, E, T are plain letters). |
| F7 / Shift+F7 | Same as N / P while the layer is open | Today's meaning (live changes) when it is closed. |
| K / R | Keep / Revert the current change | |
| Shift+K / Shift+R | Keep all / Revert all | No confirmation; Undo covers it. |
| Ctrl+Z | Undo the last Keep/Revert of this document | Only with focus in the document or the bar. In the Explorer, Ctrl+Z still undoes file operations. |
| Esc | Close the layer (back to the offer bar) | After overlays: the find bar, a popover or a menu closes first. |
| Enter / Space | On a removal marker: Show/Hide | |

The keys are matched by `matchReviewKey(e)` in `shortcuts.ts` (like `matchNotesShortcut`), not added to
`SHORTCUT_ACTIONS`, so the global keyboard map does not change. Add rows to the keyboard table in `wireframes.md`
section 13 under "Review (while reviewing)". Tooltips name the keys: `Keep (K)`, `Revert (R)`, `Next change (N)`.

## 8. Commands (Search everywhere)

| Id | Label | Available |
|---|---|---|
| `reviewChanges` | Review changes | current document status `changes` |
| `reviewKeepAll` | Keep all changes | status `changes` or `tooLarge` |
| `reviewRevertAll` | Revert all changes | status `changes` |

No default shortcut. Running `reviewKeepAll`/`reviewRevertAll` does not open the layer; it toasts with Undo.

## 9. Undo

The core has no undo today. The cheapest correct option is **a small core addition plus an Undo toast**:

- Core: a per-document, in-memory undo stack, cleared by `review_release`. Each Keep/Revert/Keep all/Revert all pushes
  `{ reviewedBefore, reviewedAfter, fileBefore (text, only for reverts), fileAfterHash }`. New command
  `review_undo(path) -> ReviewView`: for a Keep, moves the baseline back if it is still `reviewedAfter`; for a Revert,
  writes `fileBefore` through the guarded save if the file still has `fileAfterHash`; otherwise rejects `reviewStale`.
  The copy cleanup spares copies the stack points to.
- UI: every action shows a toast with **Undo** (`review.kept`, `review.reverted`, `review.keptAll`, `review.revertedAll`);
  the toast's Undo and Ctrl+Z call `reviewUndo`. Ctrl+Z repeats down the stack. No redo.
- An undo that fails with `reviewStale`: warn toast `review.undoStale`.

If the core addition is not taken, the fallback is: Revert gets an Undo toast that re-saves the pre-revert text through
the editor's guarded save (the UI reads it with `readSource` just before reverting), and Keep has no undo. Ctrl+Z is then
not bound. This is clearly worse; prefer the core stack.

## 10. Screen readers

- One polite live region inside `ReviewBar` (visually hidden). The offer bar is `role="status"` (announced once when it
  appears). The reviewing bar is `role="toolbar"`, `aria-label="Review changes"`.
- Announce: opening (`review.sr.opened`: "3 changes. Change 1 of 3, modified."), moving (`review.sr.at`), Keep
  (`review.sr.kept`), Revert (`review.sr.reverted`), undo (`review.sr.undone`), all done (`review.allDone`), refresh
  (`review.sr.updated`, rate-limited), stale (the toast is `role="alert"` already).
- Old and new blocks use `<del>` and `<ins>`; the wrapper's `aria-label` names kind and position.

## 11. Data, lifecycle and level 2 hooks

**Needed from the core (small):**
1. Line anchors: `startLine`/`endLine` (1-based, as `data-sourcepos`) on each `ReviewBlock`, and on each change a
   `newLine` (the first line of the new range; for a removal, the line it sits before; `null` at the end).
2. `hash` on `DocumentView`, so the layer decorates only when `docs.current.hash === review.hash`; otherwise it keeps the
   old decorations hidden and refetches once.
3. `review_undo` (section 9).

**Store** (`stores/review.svelte.ts`): `view: ReviewView | null`, `open: boolean`, `current: number`,
`dismissedHash: string | null`, `busy: boolean`, `entries: ReviewEntry[]` (from `review-changed`, subscribed now; level 1
reads it only to skip `reviewGet` for documents not listed).

**`review_release(path)`** is called when:
- a document drops out of the **last 5 documents visited** (a small LRU), so following a link and coming back with
  Alt+Left keeps the review;
- the project is closed or replaced (release every path in the LRU);
- never on Dismiss, Done, Esc, or entering edit mode.

**Errors** (the `error.<kind>` convention, `{subject}` is the file name):
- `reviewStale`: "{subject} changed while you were reviewing. The review is up to date; nothing was written."
- `reviewNoBaseline`: "The approved version of {subject} wasn't kept, so it can't be shown or restored."
- `reviewOff`: "Changes aren't reviewed here (notes and single files)." (Commands are unavailable there, so this only
  shows on a race.)

**Level 2 hooks** (build them as parameters now, use them later):
- `ReviewBar` takes `mode: "quiet" | "gate"`. In `gate` the offer bar is not dismissible, shows for `new` and `deleted`
  too, and reads "To review · 3 changes".
- The layer already renders `new` (one added change) and `deleted` (one removal with Restore in place of Revert).
- `entries` drives the to-review marker, the *Since last visit* count and a `nextToReview` command (reserve the id and
  `Alt+R`, free today).
- LRU release is skipped in `gate` mode (the reviewed version is persistent there).

## 12. Tokens (add to `tokens.css`)

| Token | Light | Dark |
|---|---|---|
| `--review-add-bg` | `color-mix(in srgb, var(--git-added) 9%, transparent)` | `14%` |
| `--review-del-bg` | `color-mix(in srgb, var(--git-deleted) 7%, transparent)` | `12%` |
| `--review-add-bar` | `var(--git-added)` solid 3 px at `left: -22px` | same |
| `--review-del-bar` | `var(--git-deleted)` dashed 3 px at `left: -22px` | same |
| `--review-current` | `var(--accent-muted)` 1 px ring | same |

Old text uses `var(--muted)` with `text-decoration-color: color-mix(in srgb, var(--git-deleted-text) 60%, transparent)`;
both themes keep AA on the tints. The bars sit where the live gutter sits (`-22px`); while the layer is open the live gutter
is off, so they never overlap. Transitions 150 ms; no animation under reduced motion.

## 13. i18n keys (English; add to every locale)

```ts
"review.offer.one": "Changed since you last read · {n} change",
"review.offer.other": "Changed since you last read · {n} changes",
"review.offerShort.one": "{n} change",
"review.offerShort.other": "{n} changes",
"review.review": "Review",
"review.dismiss": "Hide until it changes again",
"review.reviewing.one": "Reviewing {n} change",
"review.reviewing.other": "Reviewing {n} changes",
"review.position": "{i} / {n}",
"review.prev": "Previous change (P)",
"review.next": "Next change (N)",
"review.keep": "Keep",
"review.revert": "Revert",
"review.keepHint": "Keep (K): this change becomes part of the version you approved",
"review.revertHint": "Revert (R): put back what was there before, in the file",
"review.keepAll": "Keep all",
"review.revertAll": "Revert all",
"review.keepAllHint": "Keep all (Shift+K)",
"review.revertAllHint": "Revert all (Shift+R)",
"review.more": "More review actions",
"review.done": "Done",
"review.doneHint": "Close the review (Esc)",
"review.bar": "Review changes",
"review.kind.modified": "Changed · {i} of {n}",
"review.kind.added": "Added · {i} of {n}",
"review.kind.removed": "Removed · {i} of {n}",
"review.change": "Change {i} of {n}, {kind}",
"review.kindWord.modified": "modified",
"review.kindWord.added": "added",
"review.kindWord.removed": "removed",
"review.removedRun.one": "{n} block removed: “{preview}”",
"review.removedRun.other": "{n} blocks removed: “{preview}”",
"review.keepChange": "Keep change {i} of {n}",
"review.revertChange": "Revert change {i} of {n}",
"review.kept": "Change kept",
"review.reverted": "Change reverted in {name}",
"review.keptAll": "All changes kept",
"review.revertedAll": "All changes reverted in {name}",
"review.undo": "Undo",
"review.undone": "Undone",
"review.undoStale": "Can't undo: {name} changed since.",
"review.allDone": "All changes reviewed",
"review.stale": "{name} changed while you were reviewing. The review is up to date; nothing was written.",
"review.noBaseline": "The earlier version wasn't kept, so the changes can't be shown. They can be reviewed from the next change.",
"review.tooLarge": "This document is too large to compare (over 2 MB).",
"review.restore": "Restore",
"review.restored": "{name} restored",
"review.failed": "Couldn't review {name}: {message}",
"review.sr.opened": "{n} changes. Change 1 of {n}, {kind}.",
"review.sr.at": "Change {i} of {n}, {kind}.",
"review.sr.kept": "Kept. {left} left.",
"review.sr.reverted": "Reverted. {left} left.",
"review.sr.undone": "Undone. {n} changes.",
"review.sr.updated": "The document changed. {n} changes to review.",
"cmd.reviewChanges": "Review changes",
"cmd.reviewKeepAll": "Keep all changes",
"cmd.reviewRevertAll": "Revert all changes",
"header.reviewChanges": "Review changes",
"error.reviewStale": "{subject} changed while you were reviewing. The review is up to date; nothing was written.",
"error.reviewNoBaseline": "The approved version of {subject} wasn't kept, so it can't be shown or restored.",
"error.reviewOff": "Changes aren't reviewed here (notes and single files).",
```

`review.sr.kept`, `review.sr.reverted`, `review.sr.opened` and `review.sr.updated` need `.one`/`.other` forms in the
locales that inflect; English keeps the plural shown.

## 14. Tests the implementer should add

- `reviewLayer.test.ts` (jsdom): anchoring by lines for modified, added, removed at start/middle/end; wrapper labels;
  current-change recovery by `old.startByte` after a refresh.
- `shortcuts.test.ts`: `matchReviewKey` (N, P, K, R, Shift+K, Shift+R, Esc; none with Ctrl/Alt).
- Store: debounce of refreshes, stale handling (no repeat), LRU release calls, dismiss by hash.
- i18n test already checks every locale has every key.
