<div align="center">

<img src="docs/assets/logo.svg" width="96" height="96" alt="TrailMark logo">

# TrailMark

**A fast, beautiful Markdown IDE that keeps up with your AI agents.**

Created and maintained by [Edelmar Schneider](https://github.com/EdelmarSchneider).

Open a folder and read it like a wiki. Watch documents change live as agents write them, see exactly what changed,
and find any section in seconds. It works with any AI tool, or with none.

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
- **Navigate** a whole folder like a local wiki: relative links, back and forward, quick open.
- **Follow** agents live: changed blocks glow, removed blocks leave a marker, and a live feed shows every file being touched.
- **Review** what happened since your last visit, even if the agent already committed.
- **Find** "character descriptions" or "retry policy" across hundreds of long docs, down to the exact section.

TrailMark only relies on your **filesystem and git**. It works the same with Claude Code, opencode, Cursor, Aider, Codex or
Gemini CLI, and it's a great Markdown reader if you don't use AI at all.

## Features

### 📖 Reading that feels like a book
- GitHub-flavored Markdown: tables, task lists, footnotes, alerts (`> [!NOTE]`), description lists
- **Mermaid** diagrams, **KaTeX** math, syntax-highlighted code with copy buttons
- Frontmatter shown as a clean metadata card
- Light and dark themes, adjustable zoom and reading width, a focus mode with both side panels collapsed
- JSON, XML and plain text through an extensible renderer registry

### 🧭 Navigate like a wiki
- Open a **folder** for IDE mode (tree, search, live feed) or a **single file** for a distraction-free reader
- Relative links, anchors and images resolve inside the app, with back and forward history
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
- Semantic search with a local multilingual model is planned for v0.3 (see the [roadmap](#roadmap))

![Section search: "character descriptions" finds characters.md › Aria](docs/assets/screenshot-search.png)

### 🌿 Git-aware
- Badges for modified, added and untracked files, with folder roll-up
- Baseline **"since your last visit"** (default) or **"since HEAD"**: useful when agents commit every few minutes
- Banner for rebase or merge in progress; clear messages when git can't read a repo

### 🔁 Close the loop with your AI
- **Copy reference** (`@docs/plan.md`) and **copy selection with reference**: a quoted excerpt plus path and section,
  ready to paste into any AI chat or CLI
- Configurable reference template

### 🗂️ Zero footprint by default
TrailMark keeps its state in your user profile. If you want settings to travel with the project, run
**Initialize TrailMark in this project** to create a `.trailmark/` folder (shared `config.toml`; caches are gitignored automatically).

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

| Action | Shortcut |
|---|---|
| Quick open file | `Ctrl+P` |
| Search all docs (by section) | `Ctrl+Shift+F` |
| Find in page | `Ctrl+F` |
| Toggle left / right panel | `Ctrl+B` / `Ctrl+Alt+B` |
| Next / previous change | `F7` / `Shift+F7` |
| Back / forward | `Alt+←` / `Alt+→` |
| Copy reference (or selection with reference) | `Ctrl+Alt+C` |
| Zoom in / out / reset | `Ctrl+=` / `Ctrl+-` / `Ctrl+0` |

## Roadmap

**v0.2**
- File tags and short descriptions ("Character sheets", "NPC dialogues"…): read from frontmatter or added in the app
  without touching your files, with a Tags view and `tag:` search filters
- WYSIWYG editing (tables, images, formatting) with conflict-safe saving that never silently reformats agent-written files
- Outline panel, "unread since last read" markers, commit timeline, rendered diff against the baseline
- Rich JSON and XML views, clickable code references (`src/main.rs:42`)
- Installers with `trailmark` on `PATH` and `.md` file association

**v0.3**
- Semantic and hybrid search with a local multilingual embedding model (optional download)

**Later**
- Review comments that turn into a prompt or a `REVIEW.md` for your agent
- An MCP server so any agent can ask TrailMark "where is X described?"
- Image management, templates, backlinks and `[[wikilinks]]`, export to PDF and HTML

## Tech

TrailMark is a [Tauri 2](https://v2.tauri.app) app. The core is written in **Rust**: rendering (comrak + ammonia), file watching
(notify), git (libgit2) and the search index. The UI is **Svelte 5** + TypeScript running in the system webview. That keeps the
installer small and memory use low compared to Electron apps.

## Contributing

Issues and pull requests are welcome. The project uses **git-flow** (`develop` for integration, `feature/*` branches) and a strict
quality gate:

```bash
npm run check   # rustfmt, clippy -D warnings, cargo test, svelte-check, vitest
```

Design and planning docs are in [`docs/`](docs/). See [CONTRIBUTING.md](CONTRIBUTING.md) before opening a pull request.

## License

Copyright (c) 2026 Edelmar Schneider. Licensed under either of [MIT](LICENSE-MIT) or [Apache-2.0](LICENSE-APACHE), at your option.

"TrailMark" and its logo are trademarks of Edelmar Schneider: forks are welcome under a different name. See [TRADEMARKS.md](TRADEMARKS.md).
