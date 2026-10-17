# Review changes

Status: **approved** (both levels; rendered blocks first, a source line diff later). Origin: GitHub issue #3,
"Diff-based approval/review for AI-generated changes".

## The need

An agent edits a document and sometimes deletes it, rewrites it from scratch, makes changes nobody asked for or pads it
with repetition. The person wants to see exactly what changed and take back the parts they don't want, piece by piece,
the way a code review accepts or rejects hunks.

## What TrailMark can and cannot do

Agents write straight to disk, with any tool or none, so TrailMark **cannot hold a change before it lands**; it never
sits between the agent and the file. What it can do, tool-agnostically, is review a change **after** it lands, and undo
the parts that should not have happened:

- **Keep** a change: it becomes part of the version you approved.
- **Revert** a change: TrailMark writes the old text back into the file.

Reverting a hunk is the same outcome as rejecting it before it was applied, unless the agent has already built on it,
which the review makes visible.

## The model

Each document has a **reviewed version**: the text the person last approved. The review is a diff of the file today
against that version.

- The diff is per **Markdown block** (paragraph, heading, list, table, code block, quote...), using the block ranges the
  editor already computes. A *change* is a run of consecutive changed blocks; an edited block is paired with its old
  version (the live-reload diff already does this pairing).
- **Keep** applies the change to the reviewed version. **Revert** applies its inverse to the file. Either way the two
  texts move closer; when they are equal, there is nothing left to review.
- **Keep all** sets the reviewed version to the file; **Revert all** writes the reviewed version back to the file (this
  covers "the agent rewrote the whole file").
- **Deleted documents**: a reviewed document that disappears is listed as deleted, with **Restore** (and Keep = accept
  the deletion).
- **New documents**: shown as all-new; Keep accepts, Revert sends the file to the recycle bin (with Undo).
- **Own edits** (saving from the editor, file operations) are approved automatically: you wrote them.
- **Safe writes**: a Revert writes only if the file still has the hash the diff was computed from. If the agent wrote
  in between, the review refreshes and nothing is overwritten.

### Where the reviewed version lives

Unread markers keep only a hash per document (`read.json`). The review also needs the text, so TrailMark keeps a copy of
each document's reviewed version next to it, content-addressed by the same xxh3 hash (`.trailmark/local/reviewed/<hash>`
in an initialized project, the app's data folder otherwise), and removes copies nothing points to. Markdown is small; a
document above a size limit (e.g. 2 MB) is not copied and only shows "changed".

**First run after updating**: there are hashes but no copies yet. When the project is a git repository and the
committed version of a document has the reviewed hash, that version is the copy; otherwise the document can be reviewed
from its next change.

## Two levels

1. **Always on, quiet** (default): the reviewed version is the **last read** version, as unread markers already define it.
   Opening a changed document shows a slim bar: "Changed since you last read: 3 changes · Review". The previous version
   is kept for as long as the document stays open, so the review stays available after the document counts as read.
   Nobody who does not care sees anything new besides that bar.
2. **Review mode, per project** (opt-in, off by default): reading no longer approves. A document stays "to review" until
   every change is kept or reverted, with its own marker (distinct from the unread dot), a count in *Since last visit*,
   and *Next to review*. This is the approval gate for people who direct agents (it is the S4 "review and approve" idea of
   spec-driven work).

## The view

In the **Read** view, a review layer over the rendered document:

```
┌ spec.md ─────────────────────────────── 3 changes ‹ 1/3 › · Keep all · Revert all ┐
│ ## Goals                                                                          │
│ ┃ Old paragraph, struck through, red tint                       [Keep] [Revert]    │
│ ┃ New paragraph, green tint                                                        │
│ Unchanged text renders as usual...                                                │
│ ┃ ▸ 2 blocks removed (click to show them)                       [Keep] [Revert]    │
└───────────────────────────────────────────────────────────────────────────────────┘
```

- Rendered, not raw: tables, lists and code look like themselves. A **Source** toggle shows the same changes as a line
  diff of the text for people who want the exact characters.
- Keyboard: `N` / `P` next/previous change, `K` keep, `R` revert, `Shift+K` / `Shift+R` all; announced to screen readers.
- Every Keep/Revert is undoable (`Ctrl+Z`) while the view is open.
- Entry points: the bar on a changed document, the document `[:]` menu, *Since last visit* rows, Search everywhere
  ("Review changes").

## Agents (later, optional)

Through the agent bridge (opt-in), an agent can ask which of its changes were reverted, and a Revert can carry a short
comment ("too verbose") that becomes a comment the agent sees. The review works fully without it.

## Out of scope for the first version

Word-level highlighting inside a changed paragraph (a later refinement), reviewing non-Markdown files, three-way merges.

## Build order

1. Rust: reviewed-version store (copies, GC, migration via git), block diff with source ranges, `keep`/`revert` commands
   with hash checks, events. Fully unit-tested without UI.
2. UI: review bar and layer in Read view (level 1), keyboard, undo, i18n in all languages.
3. Review mode per project (level 2): markers, *Since last visit*, *Next to review*, deleted/new documents.
4. Source line-diff toggle; then the bridge additions.

## Implementation notes: the Rust core (`feature/review-changes`, build step 1)

Everything lives in `src-tauri/src/review/`, with no UI. The review follows the unread markers through a small observer
(`unread::ReadObserver`), so unread keeps working as before when it is not there.

### Modules

| Module | What it does |
| --- | --- |
| `diff` | `Comparison::new(reviewed, current)`: the changes, and `kept(i)` / `reverted(i)`, the two splices. Pure. |
| `book` | Per document: the reviewed version (a hash), the version held for an open review, which documents differ, which are gone and since when. Pure. |
| `saved` | `reviewed.json`: the versioned file format. Pure. |
| `store` | The copies: `reviewed/<xxh3 hex>` next to `read.json`, written atomically, checked by hash when read, cleaned up. |
| `committed` | The text of a version from the last commit of the repository (the first run after an update). |
| `undo` | The steps of Keep and Revert that can be taken back, per document, in memory. Pure. |
| `view` | The serializable review (`ReviewView`): changes with their blocks rendered by the reading view's renderer. |
| `mod` | `ReviewService`: a thread that follows the unread markers and keeps the files; the commands' logic. |

### The reviewed version and its copy

* `reviewed.json` is a map of its own. At level 1 it follows the read hash: whenever the unread markers move a
  document's read version, the reviewed version becomes it. A saved `reviewed.json` stands over `read.json` the first time
  a document is seen in a session (a change the person kept survives a restart). Level 2 (reading does not approve) is
  that follow switched off in `Book::observe`.
* The file is versioned (`{"version": 1, "documents": {"plan.md": {"hash": "...", "deletedAt": 1760000000000}}}`; hashes in
  the form `read.json` uses, `deletedAt` in milliseconds, present only while the file is gone). A file of another version
  is not read: the review starts over from what was read.
* A document seen to be gone (the disk has been looked at, and the unread markers no longer have it) is dated. After 30
  days it is forgotten and its copy is let go, and at most 200 deleted documents are remembered, the most recently
  deleted ones. A document that comes back loses its date. Names in the file that are no path of the project are never
  listed.
* A copy of the text is taken, in order of preference: when the person reads (the unread service hands over the bytes it
  just read), when the person saves (`wrote`), from the file if it still has that hash, from the last commit if its blob
  has that hash. A version with no copy only reports "changed" (`noBaseline`) until the document is read again.
  Documents over 2 MB are never copied (`tooLarge`; Keep all still works and clears them), and a version known to be
  too large, or to be lost, is not looked for again until nothing wants that version.
* Copies are a cache that is checked by its hash when read, so they are written through a temp file and a rename, with
  no flush to the disk, and the folder is made ready once for a series of them.
* Copies nothing points to are deleted a moment after the versions change; a copy written in the last two minutes is
  kept whatever points to it (so a keep that is being made cannot lose its copy), and so is a copy a step that can be
  taken back needs, and one that the `reviewed.json` on the disk points to (another instance of the program on the same
  project writes there).
* The folder is private like the rest of the local state (inside `.trailmark/local/`, covered by its `.gitignore`, never
  written behind a link).
* When the project gets a `.trailmark/` folder, the copies in use are carried to it **without the lock** the commands
  share, the ones written meanwhile follow, and a copy is deleted from the old place only when it is in the new one.

### The version held for an open review

Opening a changed document reads it, which approves what it holds now. So when the **person reads** a document and its
read version moves, the version before it is *held*: it stays the baseline, with the changes to review, until
`review_release(path)` (the UI closes the document or its review), the document is back at that version, or the app
closes. One version is held per document, in memory only, and a later reading does not replace it. Reading that is not
the person opening a document (mark all read, a folder marked read) and the person's own saves hold nothing. A save
while a version is held does not release it: the person's edit then shows among the changes.

### Keep and Revert

* Both are computed again from the texts, never from byte offsets sent by the UI: a call carries `path`, the `hash` of the
  file and the `reviewedHash` it was drawn from, and the index of the change. If either hash no longer matches, the call
  fails with kind `reviewStale` and nothing is written; the UI asks `review_get` again.
* **Keep** splices the new blocks into the baseline text, stores the result as a new copy and moves the baseline. The file
  is not touched. **Keep all** makes the file's text the reviewed version (for a deleted document: forgets it), and
  counts as **reading** the document, the way a save does (no version is held). Keeping one change does not mark the
  document read: the rest is still to review.
* **Revert** splices the old blocks into the file through the editor's guarded save (written only if the file still has
  the hash; announced as the app's own write). The reviewed version stays as it was, so the change just leaves the
  difference. The caller then tells the unread markers (`rewritten`, with the path the file system has): a document that
  was read stays read. **Revert all** writes the reviewed text, byte for byte.
* A deleted document: Restore writes the kept text back (the folders it was in are made again); Keep accepts the deletion.
  A new document: Keep approves it; Revert sends it to the system's recycle bin, and only a document the review shows as
  new can be (not one that is too large, not text, or empty). The file is looked at once more right before it goes, and
  a file that changed since is left alone (`reviewStale`). The recycle bin has no in-app undo, so the review keeps the
  text: `review_undo` writes it back.
* Splices replace only the blocks and the separator that joins them to their neighbours, so the blank lines of the rest
  of the file stay. A removed block never leaves its neighbours glued into one paragraph, and the inserted text takes the
  line ending of the file it goes into.
* **Every single Keep or Revert is checked.** The result is parsed again, and it must have exactly the blocks of the
  text being changed with that change swapped for the other side's blocks (kind and text), which is what the view showed.
  When a splice would join or split blocks (a table that takes in the paragraph next to it), it is made again with a
  blank line around the swapped blocks. When that does not give the blocks either (a line of dashes that turns the top of
  the file into front matter, a link definition whose label is taken by another), the call fails with kind
  `reviewConflict` and writes nothing: Keep all and Revert all, which are exact, remain. In a random test of 56,000
  single splices, 238 (0.4%) did not give the blocks at the first try; the blank line rescued 14, and 224 are refused
  (177 by front matter, 47 by a link label defined twice, which reads differently in the other context).

### Undo

`review_undo(path)` takes back the last Keep or Revert of the document that can still be taken back. The steps are kept
in memory per document, newest last (50 at most, and 8 MB of text), and are let go by `review_release`.

| Step | Taking it back | Fails with `reviewStale` when |
| --- | --- | --- |
| Keep, Keep all | the approved (and held) version are put back | the document no longer stands where the step left it (the reading moved it, another Keep) |
| Revert, Revert all | the text the file had is written back, through the guarded save, as the app's own write | the file no longer has the hash the step left (an agent wrote since) |
| Revert of a new document | the file is made again with its text (never over a file) | something is at its place |

A step that fails is dropped, so that the one before it can be tried. With no step left, `reviewStale` is the answer too.
The clean-up spares the copies the steps need. A document that moves takes its steps along; one that is deleted loses them
(except a new document that was sent away, which is what its step is for). Keep all and the removal of a new document are
undone like the others; Restore of a deleted document is not a step.

### What the person's own actions mean

* **Saves** are approved as before (`wrote`).
* **Files sent to the recycle bin** from the Explorer, or by a Revert of a new document, are forgotten at once, by the
  markers and the review (a move to nowhere, in the same message that says the document is gone): they are not listed as
  deleted for ever. A document an agent deletes is, with Restore.
* A file that differs from the reviewed version in **blank lines, a byte order mark, the final newline or line endings
  only** is no change to review. When `review_get` finds no block change but the hashes differ, the file as it is becomes
  the approved version (a copy is kept, any held version is released), and the answer is `unchanged` with the new
  `reviewedHash`. This holds for an empty new document too.

### Diff rules

Blocks are those of the editor, plus every run of non-blank lines the editor leaves between its blocks, which is **link
reference definitions** (`[d]: https://...`): comrak consumes them without making a node, so each run is a block of its
own (kind `other`, HTML empty: the view shows its `text`). A definition that is edited, added or removed is therefore a
change that is shown, kept and reverted like any other, instead of travelling unseen with its neighbour. Each block is
drawn together with the definitions of its own version, so that reference-style links (`[docs][d]`) are links in the view.
Two blocks are the same when their text is the same, line endings aside. Blank lines, a byte order mark and the final
newline are separators and never a change. A longest common subsequence finds the blocks that stayed; everything between
two of them is one change; inside it an edited block is paired with its old version when they share 40% of their words
(CJK counted by character). A moved block is a removal and an addition.

Lines are counted the way the reading view counts them (`data-sourcepos` of its top-level elements): the break is `\n`,
`\r\n` or a lone `\r`, and a byte order mark is not part of the first line.

### Following the unread markers

* The unread side tells the review only when something changed (the tracker counts its changes), and the review thread
  takes the messages that piled up behind a long piece of work (the first run, copying many documents) as one: the moves
  in order and the newest documents. Messages of other kinds are never taken out of their order.
* A reading of a document that is read already, or of one the project does not have, is not kept waiting for a read that
  never comes.

### Commands and events

| Command | Arguments | Answer |
| --- | --- | --- |
| `review_list` | | `ReviewEntry[]`: documents that differ (`changed`, `new`, `deleted`) |
| `review_get` | `path` | `ReviewView` |
| `review_keep`, `review_revert` | `path`, `hash`, `reviewedHash`, `index` | `ReviewView` after |
| `review_keep_all`, `review_revert_all`, `review_restore_deleted` | `path`, `hash`, `reviewedHash` | `ReviewView` after |
| `review_undo` | `path` | `ReviewView` after |
| `review_release` | `path` | nothing |

Payloads the UI relies on:

* `ReviewBlock` (in `rows[].old` / `rows[].new`): `index`, `startByte`, `endByte`, **`startLine`, `endLine`** (1-based,
  as `data-sourcepos`), `text`, `html` (empty when the block draws nothing or could not be drawn).
* `ReviewChange`: `kind`, `old`, `new` (byte ranges), `oldBlocks`, `newBlocks`, **`newLine`** (the first line of its new
  blocks; for a removal, the line of the block it sits in front of; `null` at the end of the file), `rows`.
* `ReviewView`: `path`, `status`, `hash`, `reviewedHash`, `restorable`, `changes`.
* `DocumentView.hash` (from `read_document`): the hash of the file's bytes as it was read, 16 hex digits, the same as
  `ReviewView.hash`; `null` for pictures, sounds, videos and files too large to be read.

Event `review-changed`: the whole `ReviewEntry[]`, sorted, each time it changes. It is cheap (hash comparisons).

Errors (`kind`): `reviewStale`, `reviewNoBaseline`, `reviewConflict` (a single Keep or Revert that cannot be applied
alone), `reviewOff` (the notes, a single file), plus the editor's `notMarkdown`, `tooLarge`, `encoding`, `denied` and the
vault's path errors.
