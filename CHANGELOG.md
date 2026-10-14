# Changelog

All notable changes to TrailMark are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Explorer filter
- A filter in the Explorer header chooses what the tree shows: Documents, Docs and assets (the default), Everything, or Custom
  (file-type checkboxes plus optional `+pattern` / `-pattern` globs such as `+*.sql -archive/**`). Folders with no visible file are
  hidden, so a code repository shows `docs/` and the root documents instead of every source folder.
- The tree footer says how many files are hidden ("Showing 52 of 1,204 files") and offers **Show all** for the moment. A hidden file
  you open by link, search, Ctrl+P or the feed stays in the tree, dimmed, while it is open.
- Quick open follows the same filter and ends with "+N hidden files match", which lists them.
- Your choice is remembered per project. A project can set the default for everyone in `.trailmark/config.toml`
  (`[tree] show = "docs-and-assets"`, `include = [...]`, `exclude = [...]`); your own choice wins.

### Tags and descriptions
- Tag any file and give it a one-line description. Tags written in a document's frontmatter (`tags:`) are read automatically and
  shown with a file glyph; tags added in the app are kept outside your files, in `.trailmark/meta.toml` once the project is
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
- "Since last visit" shows what changed while TrailMark was closed, even after commits.
- Linked git worktrees are discovered and watched.

### Search
- Section-level full-text search (`Ctrl+Shift+F`): `file › heading › snippet` results that jump to the exact section.

### Git
- Status against a baseline of your last visit (the default) or HEAD; a repo-state banner; clear errors.

### Closing the loop
- Copy path, copy reference (template, default `@{path}`), and copy selection with reference.
- Open in your default app, and show in the file manager.

### Project
- Optional `.trailmark/` project folder for shared settings; zero footprint by default.
- In-app updates through GitHub Releases, with signed update packages.
