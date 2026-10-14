<div align="center">

<img src="docs/assets/logo.svg" width="96" height="96" alt="TrailMark logo">

# TrailMark

**A fast, beautiful Markdown IDE that keeps up with your AI agents.**

Created and maintained by [Edelmar Schneider](https://github.com/EdelmarSchneider).

Open a folder and read it like a wiki. Watch documents change live as agents write them, see exactly what changed,
find any section in seconds, and edit with a visual editor that only rewrites the blocks you touch.
It works with any AI tool, or with none.

[Features](#features) · [Install](#install) · [Usage](#usage) · [Shortcuts](#keyboard-shortcuts) · [Roadmap](#roadmap) · [Contributing](#contributing)

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/screenshot-dark.png">
  <img src="docs/assets/screenshot-light.png" alt="TrailMark in IDE mode: a design doc with a live-change glow, a removed-block marker and the live changes feed">
</picture>

</div>

---

## Why TrailMark?

AI coding agents write a lot of Markdown: specs, plans, research notes, reports. Reading that as raw text in a terminal or an
editor is tiring. Browser extensions render one file at a time and don't show what changed. TrailMark closes the gap between
**the documents your agents produce** and **you**, the human who has to read, check and steer them.

- **Read** beautifully rendered Markdown, with Mermaid, math, alerts, footnotes and frontmatter.
- **Navigate** a whole folder like a local wiki: relative links, back and forward, quick open, an outline of every document.
- **Follow** agents live: changed blocks glow, removed blocks leave a marker, and a live feed shows every file being touched.
- **Review** what happened since your last visit, even if the agent already committed.
- **Find** "character descriptions" or "retry policy" across hundreds of long docs, down to the exact section.
- **Organize** files with tags and one-line descriptions, without touching the files themselves.
- **Edit** in a visual or a source editor that rewrites only the blocks you changed, and never loses your work to an agent.

TrailMark only relies on your **filesystem and git**. It works the same with Claude Code, opencode, Cursor, Aider, Codex or
Gemini CLI, and it's a great Markdown reader and editor if you don't use AI at all.

## Features

### 📖 Reading that feels like a book
- GitHub-flavored Markdown: tables, task lists, footnotes, alerts (`> [!NOTE]`), description lists
- **Mermaid** diagrams, **KaTeX** math, syntax-highlighted code with copy buttons
- Frontmatter shown as a clean metadata card
- Light and dark themes, adjustable zoom, and a focus mode with both side panels collapsed
- Three **reading widths**: Comfortable (a book-like column), Wide and Full. Wide tables, diagrams and images take the room
  they need while the text stays at a comfortable measure
- JSON, XML and plain text through an extensible renderer registry

### 🧭 Navigate like a wiki
- Open a **folder** for IDE mode (tree, search, live feed) or a **single file** for a distraction-free reader
- Relative links, anchors and images resolve inside the app, with back and forward history
- An **Outline** of the open document's headings (`Ctrl+Alt+O`): the section you're reading is marked, click a heading to jump,
  collapse sections you don't need. It keeps working while you edit, and a dot marks the sections an agent just changed
- A **filter** for the Explorer (Documents, Docs and assets, Everything, or your own patterns) keeps a code repository's tree to the
  documents that matter, and tells you how many files it hides
- The Explorer **follows the file you open**, however you got there: quick open, a link, search, the feed, back and forward
- Quick open (`Ctrl+P`), find in page (`Ctrl+F`), session restore and recent projects
- One process per project: `trailmark ~/code/my-project`

### ⚡ Follow your agents live
- Documents reload **without losing your scroll position**
- **Changed blocks glow** and fade; **removed blocks leave a marker** you can expand
- **Live changes** feed: one entry per file, bursts grouped, `git checkout` storms collapsed into one line
- **Follow mode** automatically opens the doc the agent is writing (rate-limited, never jumps while you're selecting)
- Git worktrees are discovered automatically, so changes made by agents working in parallel branches show up too

### 🔍 Find anything, to the section
- Section-level full-text search (`Ctrl+Shift+F`): results show `file › heading › snippet` and jump to the exact spot
- Accent- and case-insensitive, with light stemming, and incremental updates as files change
- Filter by tag right in the search box (`tag:needs-review retry policy`)
- Semantic search with a local multilingual model is planned for v0.3 (see the [roadmap](#roadmap))

![Section search with a tag filter: "lanterns" in the files tagged needs-review, each file with its description, and the match highlighted in the open document](docs/assets/screenshot-search.png)

### 🏷️ Tags and descriptions
- Give any file a **one-line description** and **tags** ("needs-review", "character sheets"). Tags in a document's frontmatter
  are read automatically; the ones you add in the app **never touch your files**
- Added tags live in `.trailmark/meta.toml` once the project is initialized (plain text you can commit, so teammates and agents
  can read it), or in the app's own storage before that. Renaming or moving a file keeps its tags
- A **Tags view** (`Ctrl+Shift+T`) lists every tag with its number of files. Pick one, or `Ctrl+click` several to narrow the list
- Descriptions and tags show up where you look for files: tree tooltips, quick open, search results

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/screenshot-tags-dark.png">
  <img src="docs/assets/screenshot-tags.png" alt="The Tags view: every tag with its file count, the needs-review tag selected, and the three files that carry it, each with its description and git badge, next to an open document with its description and tag chips">
</picture>

### ✏️ Edit without fear
- Two editors, one click apart (`Ctrl+Alt+E`): **Visual**, a WYSIWYG editor with a toolbar, a `/` block menu, tables, task lists and
  code blocks, and **Source**, a Markdown editor with highlighting and line numbers. Both are loaded only when you start editing
- **Only the blocks you changed are rewritten.** Every other byte of the file stays as its author (or agent) wrote it, including line
  endings and a byte-order mark. A block you edited carries a bar in the margin until you save, and the status bar counts them
- Blocks the visual editor can't write back faithfully (front matter, raw HTML, math, diagrams, footnotes) stay as they are,
  with an **Edit in Source** button that opens the Markdown at that block
- **Agents and you can share a file safely.** If something changes the file while you edit, a clean buffer reloads in place, and
  one with your edits asks: **Reload theirs** or **Keep mine**. A save never overwrites a version you haven't seen
- Unsaved work is never lost quietly: a mark in the header and the tree, a `*` in the window title, and a Save / Discard / Keep editing
  prompt before you leave
- **New file** (`Ctrl+N`): type a name in the tree, and it opens for editing with the caret after the title
- **Images** pasted, dropped or picked are saved next to the document in an `assets/` folder and linked with a relative path

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="docs/assets/screenshot-editor-dark.png">
  <img src="docs/assets/screenshot-editor.png" alt="The Visual editor: a toolbar with bold active, a table and a paragraph with one block marked as unsaved, a Mermaid diagram shown as a locked block with an Edit in Source button, and the Outline tab on the right">
</picture>

### 🔗 File operations that keep your links working
- **Rename** (`F2`), **move** (drag and drop, or `Ctrl+X` / `Ctrl+V`), **delete** (`Del`, to the system trash, after asking) and
  **New folder**, from the keyboard, the context menu or the `[:]` menu. Select several files with `Ctrl+click` or `Shift+click`
- **Links are updated for you.** Moving or renaming a file or folder rewrites the links and images that point to it, and the
  relative links inside the files that move. "Update 7 links in 4 files?" shows every change first: **Apply**, **Move without
  updating** or **Cancel**. Only the destination changes; code is never touched
- **Undo** (`Ctrl+Z` in the Explorer, or the toast's Undo button) puts back the files and every rewritten link. It is conservative:
  a document that changed since is not overwritten, and you're told what couldn't be restored
- Tags follow the files, and the live feed shows the whole operation as one row

### 🌿 Git-aware
- Badges for modified, added and untracked files, with folder roll-up
- Baseline **"since your last visit"** (default) or **"since HEAD"**: useful when agents commit every few minutes
- Banner for rebase or merge in progress; clear messages when git can't read a repo

### 🔁 Close the loop with your AI
- **Copy reference** (`@docs/plan.md`) and **copy selection with reference**: a quoted excerpt plus path and section,
  ready to paste into any AI chat or CLI
- Configurable reference template

### 🗂️ Zero footprint by default
TrailMark keeps its state in your user profile. If you want settings and tags to travel with the project, run
**Initialize TrailMark in this project** to create a `.trailmark/` folder (shared `config.toml` and `meta.toml`; caches are gitignored automatically).

## Install

> Prebuilt installers for Windows, macOS and Linux are published on the [Releases](../../releases) page.
> TrailMark updates itself: when a new version is out, you'll be offered a one-click restart to update.

**Build from source** (requires [Rust](https://rustup.rs), [Node.js](https://nodejs.org) 20+ and the
[Tauri prerequisites](https://v2.tauri.app/start/prerequisites/)):

```bash
git clone https://github.com/EdelmarSchneider/trailmark.git
cd trailmark
npm install
npm run tauri build
```

## Usage

```bash
trailmark                  # welcome screen with recent projects
trailmark .                # open the current folder in IDE mode
trailmark docs/plan.md     # open a single file in simple mode
```

You can also drag and drop a folder or a `.md` file onto the window.

## Keyboard shortcuts

On macOS, use `Cmd` where this table says `Ctrl`.

**Reading and navigating**

| Action | Shortcut |
|---|---|
| Quick open file | `Ctrl+P` |
| Search all docs (by section) | `Ctrl+Shift+F` |
| Find in page | `Ctrl+F` |
| Show the Explorer / the Tags view | `Ctrl+Shift+X` / `Ctrl+Shift+T` |
| Show the Outline (press again to go back) | `Ctrl+Alt+O` |
| Cycle the reading width | `Ctrl+Alt+W` |
| Toggle left / right panel | `Ctrl+B` / `Ctrl+Alt+B` |
| Next / previous change | `F7` / `Shift+F7` |
| Follow mode | `Ctrl+Shift+L` |
| Back / forward | `Alt+←` / `Alt+→` |
| Copy reference (or selection with reference) | `Ctrl+Alt+C` |
| Edit tags and description | `T` |
| Zoom in / out / reset | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` |

**Editing**

| Action | Shortcut |
|---|---|
| Start editing | `E` or `Ctrl+E` |
| Done (back to reading) | `Ctrl+E` or `Esc` |
| Switch between Visual and Source | `Ctrl+Alt+E` |
| Save | `Ctrl+S` |
| New file | `Ctrl+N` |
| Bold / italic / strikethrough | `Ctrl+B` / `Ctrl+I` / `Ctrl+Shift+X` |
| Link / inline code | `Ctrl+K` / ``Ctrl+` `` |
| Headings 1 to 4 / paragraph (Visual) | `Ctrl+Alt+1` … `4` / `Ctrl+Alt+0` |
| Block menu (Visual) | `/` in an empty paragraph |

**In the Explorer**

| Action | Shortcut |
|---|---|
| Rename | `F2` |
| Delete (asks first) | `Del` |
| Cut / paste to move | `Ctrl+X` / `Ctrl+V` |
| Select several files | `Ctrl+click` / `Shift+click` |
| Undo the last file operation | `Ctrl+Z` |

## Roadmap

**v0.2: the Markdown IDE** (released)
- Tags and descriptions, the Explorer filter, reading widths, an Explorer that follows the open file, and the Outline
- A visual and a source editor with block-preserving saves, safe sharing of a file with agents, new files and images
- File operations (rename, move, delete) that update links and can be undone

**v0.2.1: translations**
- Português (Brasil), Español and Deutsch

**v0.3**
- Search everywhere (`Ctrl+T`): files, sections, tags and commands in one box
- Semantic search with a local, offline, multilingual model, so "retry policy" finds the section that describes it whatever words it uses
- Unread markers ("changed since I last read it") and clickable code references (`src/main.rs:42`)
- "Open with TrailMark" in your file manager

**Later**, no promises
- Spec-driven workflows, review comments that turn into a prompt for your agent, and an MCP server so any agent can ask TrailMark
  "where is X described?"

## Tech

TrailMark is a [Tauri 2](https://v2.tauri.app) app. The core is written in **Rust**: rendering (comrak + ammonia), file watching
(notify), git (libgit2), the search index and the block-preserving saves. The UI is **Svelte 5** + TypeScript running in the system
webview, with **CodeMirror 6** (Source editor) and **Milkdown** (Visual editor). That keeps the installer small and memory use
low compared to Electron apps.

## Contributing

Issues and pull requests are welcome. The project uses **git-flow** (`develop` for integration, `feature/*` branches) and a strict
quality gate:

```bash
npm run check   # rustfmt, clippy -D warnings, cargo test, svelte-check, vitest
```

Design notes are in [`docs/design/`](docs/design/). See [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

## License

Copyright (c) 2026 Edelmar Schneider. Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

"TrailMark" and its logo are trademarks of Edelmar Schneider: forks are welcome under a different name. See [TRADEMARKS.md](TRADEMARKS.md).
