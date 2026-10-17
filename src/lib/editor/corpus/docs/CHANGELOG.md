# Changelog

All notable changes to PerMark are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Editing (Source mode)
- **Edit** a Markdown document (`E`, Ctrl+E or the Edit button) in a source editor with Markdown highlighting, line numbers and soft wrap at the reading
  width; **Ctrl+S** saves, **Done**, Ctrl+E or Esc goes back to reading. PerMark writes the text as you typed it and keeps the file's BOM and line
  endings: lines you did not touch keep their own bytes, also in a file that mixes CRLF, LF and a lone CR, and a save without an edit writes the very same
  file. Saving keeps the file's permissions. The editor is loaded only when you start editing. Ctrl+B, Ctrl+I, Ctrl+K, Ctrl+` and Ctrl+Shift+X wrap the selection in Markdown.
- Unsaved work is never lost quietly: an `Unsaved` mark in the header, a dot on the file in the tree and a `*` in the window title; leaving the document
  (closing the window, or **Restart to update**) while it has unsaved changes asks to Save, Discard or Keep editing, and Follow never jumps away while you edit.
- If someone else (an agent, another editor) changes the file while you edit, a clean buffer reloads in place and a buffer with your edits shows a banner:
  **Reload theirs** or **Keep mine**; a file deleted meanwhile can be saved again or discarded. A save never overwrites a version you have not seen: the file is compared once more right before it is replaced, so
  an agent that saves while your save is being written makes it a conflict. A save
  that fails (read-only file, access denied) keeps your text and offers Retry and Copy all.
- Your own saves are not news: no glow, toast, feed entry or Follow jump for what you just wrote, while the search index and the git badge still catch up.
- **Ctrl+N** creates a new Markdown file: type its name in the tree (`.md` is added), and it opens for editing with the caret after the title. Pasting or
  dropping an image into the editor saves it next to the document in `assets/` and inserts the link.
- Visual (WYSIWYG) editing is next; its switch is in the header, disabled for now. Files that are binary, over 5 MB, not Markdown or not UTF-8 cannot be edited
  (the Edit button says why). "Open in external editor" moved to the `...` menu.

### Outline
- The right panel has two tabs, **Changes** (the live feed and "Since last visit", as before) and **Outline**. The Outline lists the
  open document's headings, indented by nesting; the section you are reading is marked and the list follows you as you scroll.
  Click or press Enter on a heading to jump to it (it is tinted for a moment); Left and Right collapse and expand a section, and
  the collapsed state is kept per document while the app is open, even when the document is rewritten under you.
- When an agent rewrites the document, a dot marks the sections whose blocks changed (a collapsed section shows it when something
  inside it changed), and goes away with the change bars. A dot on the Changes tab says the feed moved while you were on the Outline.
- **Ctrl+Alt+O** shows the Outline (opening the panel if it is collapsed); press it again to go back to the previous tab. In
  Simple mode it opens the outline as a popover under the header. Long documents show the first 1,000 headings.
- The tab you were on is remembered per project.

### Reading width
- Three reading widths instead of two: **Comfortable** (760 px, the default), **Wide** (1100 px) and **Full** (the whole document
  pane). One click on the status bar width button, the menu entry or Ctrl+Alt+W moves to the next; Settings has all three. The
  width is remembered per project, and a project that used the old "Wide" setting keeps it.
- Tables, code blocks, Mermaid diagrams, display formulas and wide images take the room they need, up to the pane, while the text
  stays at the comfortable measure. A narrow table or a short snippet stays where it was; glow, change bars and find highlights
  follow the block.

### Explorer follows the open file
- Whatever opens a document (Quick open, search, a link, the feed, Follow, back and forward, the restored session), the Explorer
  opens the folders above it, selects it and scrolls it into view by the shortest way, without taking the keyboard focus. Worktree
  files appear under their worktree, and a file the filter hides is shown dimmed while it is open.
- **Always select the opened file** (Explorer `[:]` menu, on by default) turns it off; "Reveal open doc" still works.

### Explorer filter
- A filter in the Explorer header chooses what the tree shows: Documents, Docs and assets (the default), Everything, or Custom
  (file-type checkboxes plus optional `+pattern` / `-pattern` globs such as `+*.sql -archive/**`). Folders with no visible file are
  hidden, so a code repository shows `docs/` and the root documents instead of every source folder.
- "Only assets next to documents" (on by default) keeps the JSON, TOML, YAML, XML and image files of code folders out of the
  tree: such a file shows only when its folder, or the one above it, holds a document.
- The tree footer says how many files are hidden ("Showing 52 of 1,204 files") and offers **Show all** for the moment. A hidden file
  you open by link, search, Ctrl+P or the feed stays in the tree, dimmed, while it is open.
- Quick open follows the same filter and ends with "+N hidden files match", which lists them.
- Your choice is remembered per project. A project can set the default for everyone in `.permark/config.toml`
  (`[tree] show = "docs-and-assets"`, `include = [...]`, `exclude = [...]`); your own choice wins.

### Tags and descriptions
- Tag any file and give it a one-line description. Tags written in a document's frontmatter (`tags:`) are read automatically and
  shown with a file glyph; tags added in the app are kept outside your files, in `.permark/meta.toml` once the project is
  initialized (committable, so teammates and agents can read it) or in the app's own storage before that.
- A strip at the top of the document shows the description and the tags; `T` (or the menu) edits them, with autocomplete.
- A Tags view (`Ctrl+Shift+T`) lists every tag with its number of files; pick one, or Ctrl+click several, to list the files.
- `tag:name` filters in Quick open and in Search; descriptions show in the tree tooltip, Quick open and search results.
- Renamed files keep their tags; entries of deleted files are kept until you choose "Clean up".
- A `meta.toml` that cannot be read (for example a merge conflict) makes the app's tags read-only and says which line is wrong.

## [0.1.0] - 2026-10-14

The first public release.

### Reading
- GitHub-flavored Markdown with tables, task lists, footnotes, alerts and description lists; Mermaid diagrams, KaTeX math,
  syntax highlighting, a frontmatter card, safe embedded HTML, and Unicode-safe heading anchors.
- JSON, XML and plain-text views; binary and very large files are detected; UTF-8 with BOM, UTF-16 and CRLF are handled.
- Light, dark and system themes; zoom; normal or wide reading width; collapsible and resizable side panels.

### Navigation
- IDE mode for folders (file tree that respects `.gitignore`, with git badges) and Simple mode for single files.
- In-app links and anchors with back and forward; images from the vault; external links open in the browser.
- Quick open (`Ctrl+P`), find in page (`Ctrl+F`), recent projects, and session restore.

### Following agents live
- Live reload that keeps your scroll position; changed blocks glow, and removed blocks leave an expandable marker. `F7` jumps between changes.
- A live changes feed with one entry per file, grouped bursts, git operations collapsed into one entry, a "Docs only" filter, and a toast.
- Follow mode opens the document being written (rate-limited).
- "Since last visit" shows what changed while PerMark was closed, even after commits.
- Linked git worktrees are discovered and watched.

### Search
- Section-level full-text search (`Ctrl+Shift+F`): `file › heading › snippet` results that jump to the exact section.

### Git
- Status against a baseline of your last visit (the default) or HEAD; a repo-state banner; clear errors.

### Closing the loop
- Copy path, copy reference (template, default `@{path}`), and copy selection with reference.
- Open in your default app, and show in the file manager.

### Project
- Optional `.permark/` project folder for shared settings; zero footprint by default.
- In-app updates through GitHub Releases, with signed update packages.
