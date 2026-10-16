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
