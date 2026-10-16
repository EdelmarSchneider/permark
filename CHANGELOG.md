# Changelog

All notable changes to TrailMark are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **"Open with TrailMark" on folders in the Microsoft Store version.** Right-click a folder, or the background of a folder, and choose
  **Open with TrailMark**; on Windows 11 it is in the main menu, not under *Show more options*. The Store package now carries a small
  Explorer component for it (the one thing a package cannot do with the registry), worded in your Windows display language
  (English, Português, Español or Deutsch). Select several folders and each opens in its own window. **Settings → File manager** says
  that the entry comes from the Store package.

### Changed
- **A new note is a note at once.** `+`, `Ctrl+Shift+N` and **New note** now always make a new note, even when the one on screen is still empty,
  like the `+` of any editor. The note is a real (empty) file as soon as it exists, so it is in the list as "Untitled note", is still there after
  a restart, and nothing removes it for you: you delete empty notes yourself, with Delete and Undo like any other note. Several empty notes can
  exist at the same time. Pin, Rename, Share, Move to project and Delete work on a note with no text.
- **Notes folder wording.** The message about a missing notes folder now says the folder *you chose* is not there, and why TrailMark does not
  make an empty one in its place.

### Fixed
- **No question about the default notes folder.** TrailMark makes `Documents/TrailMark Notes` (and the folders above it) by itself the first
  time notes are needed, instead of asking whether to create it. A notes folder you chose in **Settings → Notes** that has gone missing (an
  unplugged drive, a renamed folder) is still reported, with **Locate...** and **Create it**, and is never created quietly.
- **One window, not two.** **New note** or **Notes** on the welcome screen, `Ctrl+Shift+N`, `Ctrl+Shift+D`, and a file opened from the welcome
  screen used to leave the welcome window behind and open the Notes window on top of it. The welcome window now turns into the Notes window, in
  the same place and size; when a Notes window is already open, it comes forward and the welcome window closes. A window with a project open
  does not change.
- **Visual mode: a block that refers to a footnote no longer carries the footnote.** A paragraph, list or table with a footnote reference was
  drawn with the footnote's text and a rule under it inside the same block, with a large empty gap. The reference is now a small marker
  (with the footnote's label), the footnote stays a block of its own where the file has it, and nothing is repeated. Saving is unchanged:
  these blocks keep their exact text, and so do the footnote definitions.

## [0.5.0] - 2026-10-15

Notes: open TrailMark, type, close. Notes save themselves, sticky notes stay for months, and Markdown files from anywhere open in one place.

### Added
- **Notes.** A window of its own for quick notes, drafts and prompts: open it with `Ctrl+Shift+D` (or the sticky-note icon, or **Notes** on
  the welcome screen), press `Ctrl+Shift+N` from any TrailMark window to start a note, and type. Notes are plain Markdown files in
  `Documents/TrailMark Notes` (**Settings → Notes** changes the folder, and **On start** can open the Notes window instead of the
  welcome screen). A note is saved as you type, and closing never asks; it has no file until you type something, and is titled by its first line.
  The list has pinned notes (drag or `Alt+↑` / `Alt+↓` to order them, `Ctrl+1` … `9` to open them), then Today, Yesterday, This week, This month
  and Older, and a filter. **Copy** (`Ctrl+Shift+C`) puts the exact Markdown on the clipboard and marks the note, with **Delete note** in the toast
  and **Delete copied notes** in the menu; in a project, `Ctrl+Shift+C` copies the whole open document. Delete goes to the system trash with Undo,
  **Show earlier versions** restores an earlier text, and **Rename** (`F2`) gives a note a file name.
  A note changed on disk while you write is never overwritten (your text is kept as a "(conflict ...)" copy), a save is durable
  (the folder is flushed after the rename), and a second TrailMark process opens the same notes read-only.
- **Files in the Notes window.** A Markdown or text file opened with "Open with TrailMark", `Ctrl+Shift+O` or a drop on the Notes window is
  listed under **Files** (a file inside the project you have open still opens in that project, and a file of the notes folder opens as a
  note). A file is never written on its own: it opens in Read, `E` edits it, `Ctrl+S` saves it, and what you type before is kept apart and
  comes back after a restart. When the file changes on disk, what you see stays until you choose **Reload** (your text is kept as a note) or
  **Keep mine** (the next save asks once more); a file that is gone stays in the list, dimmed, with **Locate...** and **Remove from list**,
  and a rename in its folder is followed. `.txt` files are edited in Source. A setting reloads a changed file that nothing was typed into.
- **Move a note to a project, and share a note with agents.** **Move to project...** (`Ctrl+Shift+S`) writes a note, exactly as it is, where the
  Save dialog says (it opens in the project you used last), sends the note to the trash and lists the copy under **Files**; the toast has
  **Open in project** and **Undo**. **Share with agents** marks a note, and an agent connected to any project whose connection is on can list
  the shared notes (`list_notes`) and read one (`read_note`); it sees no other note, and **Stop sharing** takes it back at once. The Agent
  popover lists "Read the note ..." like any other request.
- **A Microsoft Store build.** TrailMark can now be packaged for the Microsoft Store (`scripts/build-msix.ps1` builds an unsigned
  MSIX; see `docs/msstore.md`). The Store version is the same program with a few differences: the Store updates it, so it has no
  "Check for updates" and makes no update requests; it puts `trailmark` on your `PATH`, so the agent setup lines in Settings are the
  short ones (`trailmark mcp`); and Markdown files can be opened with it from the "Open with" menu. The right-click "Open with
  TrailMark" entry on folders is not in the Store version.
- A [privacy policy](https://edelmarschneider.github.io/trailmark/privacy.html) page on the website.
- **Window titles that tell windows apart.** The title of a window, which the taskbar, `Alt+Tab` and the taskbar thumbnails show, now names
  the open document and its project (`plan.md - my-project - TrailMark`), a single file (`proposal.md - TrailMark`) or the note in the
  Notes window (`Shopping list - Notes - TrailMark`). A `*` in front still means unsaved edits, and a file open in the Notes window gets it too.
  It follows the document you switch to, a rename and the language.

### Fixed
- Visual editing keeps the keyboard focus and `Ctrl+Z` across a save. A save used to load the document again: after `Ctrl+S` (and after every
  autosave of a note) the characters typed next were lost, undo stopped at the last save, and a space typed just before the save
  (`alpha ` then `beta`) was dropped, so the words joined.
- `C` (comment), `E` (edit) and `T` (tags) work right after a result of Search everywhere is opened; they used to do nothing until `Esc`.
- A save works for a file whose path is longer than 260 characters on Windows. Since 0.4.0 it failed with "path not found".

## [0.4.0] - 2026-10-15

Your AI agent can see what you are reading, and you can leave it comments on passages.

### Added
- **Agent connection.** TrailMark tells an AI agent what you are looking at, over the Model Context Protocol (MCP), so it works with
  any tool that supports MCP. Turn it on per project in **Settings → Agent connection**, which shows the setup lines for your agent
  with the program's path filled in, then add TrailMark as an MCP server (`<path to TrailMark> mcp`). The agent can ask for the open
  document, the section on screen and your selection (`get_context`), the documents that changed since you read them (`list_unread`),
  "where is X described?" (`search`), and your comments (`list_comments`, `resolve_comment`). It can also show you a place (`open`)
  without taking the focus from its terminal. It only reads, shows a place and checks off your comments: it cannot create comments or edit
  documents, and it sees only the project you opened. Most tools still work when TrailMark is closed, and `open` starts it.
- **`trailmark context`** prints where you are as one short line, for tools that add it to every prompt from a hook. While the
  connection is on, a project with `.trailmark/` also gets the same answer as a file, `.trailmark/local/agent.json`.
- **Agent** in the status bar while the connection is on, with a dot that fills while an agent is connected. Click it to see what is
  shared and what was asked lately, or to turn the connection off.
- **Comments.** Select a passage and press `C` (or **Comment** in the selection toolbar) to leave a comment. A bar marks the passage in
  the margin, and the **Comments** tab of the right panel lists the open comments by document. Comments follow their passage when the
  document changes, and a comment whose passage was rewritten is marked **Passage changed**. They are personal (kept in
  `.trailmark/local/`, never committed), work without the agent connection, and an agent can mark one done with a short reply.

### Fixed
- After using **Save** in the toolbar while the "Unsaved changes" bar was showing, the single-letter shortcuts (`T`, `E`, `Alt+U`,
  `C`) did nothing and `Esc` no longer left the editor until the window was reloaded.
- Selecting text across a heading no longer includes the `#` of the heading's link when you copy it with a reference or an agent
  asks for your selection.

### Security
- The connection is off by default and its switch is kept in TrailMark's own data folder, so nothing inside a project, such as a
  cloned repository, can turn it on. TrailMark only talks to programs on your computer, and `trailmark mcp` checks that what answers
  is TrailMark running as you before it sends anything.
- `.trailmark/local/` and `.trailmark/cache/` now ignore themselves in git, and TrailMark keeps no state in a `.trailmark` or `local`
  folder that is a link to somewhere else.

## [0.3.0] - 2026-10-15

One box to find anything, a dot for every document that changed since you read it, code references you can click, and
"Open with TrailMark" on macOS and Linux.

### Added
- **Search everywhere (`Ctrl+T`).** One box finds files, sections, tags and commands, grouped in that order, with **All**, **Files**,
  **Sections**, **Tags** and **Commands** scopes (`Tab` / `Shift+Tab` to move between them, or click). `Ctrl+P` opens it on Files and works
  exactly as Quick open did; `Ctrl+Shift+P` opens Commands. Type `>` first for commands or `#` for sections. Sections come from the same
  index as the Search panel and open at the heading; Enter on a tag puts it in the box as a `tag:` filter. Commands cover every action of the
  app, including the ones that only lived in a menu (Initialize TrailMark, Check for updates, New folder...), show their shortcut, appear only
  when they apply, and can be found by their English name in any language. A query of several words matches files whose path has every word.
- **Unread markers.** A document that changed since you last read it gets a small dot in the Explorer, in Search everywhere, in the Search
  panel, in the Tags view and in **Since last visit**; a folder with unread documents inside is shown in semibold, with "N unread" as its
  tooltip. A document counts as read once it has been on screen in a focused window for 3 seconds or you scrolled it, and your own saves
  count as read. On the first visit every existing document counts as read. **Alt+U** opens the next unread document (newest change first).
  The Explorer menu has **Mark all read** and **Show only unread**, a folder's menu has **Mark folder read**, and the document menu has
  **Mark as unread**. Moves and renames made in TrailMark keep what you have read. The state is kept per project with the rest of its
  private state (`.trailmark/local/read.json` once the project is initialized), never in a file you commit.
- **"Open with TrailMark" on macOS and Linux.** On macOS, TrailMark is listed in Finder's **Open With** for Markdown files (without becoming
  the default app), and a folder dropped on its Dock icon opens as a project. On Linux, the `.deb` and `.rpm` declare Markdown files, so
  file managers list TrailMark under **Open With**.
- **Clickable code references.** A path in inline code (`` `src/main.rs` ``, `` `src/main.rs:42` ``, `` `lib.rs#L10-L20` ``) or in plain text with a
  line (src/main.rs:42:7) becomes a link when it names a file of the project: it is looked up from the project root, then from the document's
  folder, then by file name (a name that several files share asks which one on click). A Markdown file opens in TrailMark at the line, with the
  section tint; any other file opens in your editor at the line. `Ctrl`+click on a Markdown reference uses the editor too, and the right-click
  menu has Open, Open in external editor, Copy path and Copy reference. Prose that only looks like a reference (versions, times, URLs, drive
  paths), code blocks and existing links are left alone.
- **External editor setting.** Settings has a new **External editor** field with presets for VS Code, Cursor, Zed, Sublime Text and JetBrains IDEs,
  or any command of your own (`code -g {path}:{line}:{col}`). The command is split into arguments before the file is filled in, so a path with
  spaces is always one argument, and it runs without a shell. **Open in external editor** (`Ctrl+Shift+E`, the menus) uses it too, at the block
  at the top of the view; with no command it opens the file with the system's default app as before. If the command can't be run, a message says
  which program was not found and takes you to Settings.

### Security
- The external editor starts in your home folder, never in the project, so a program placed in a cloned project cannot stand in for a
  command the editor runs. On macOS and Linux, a file without an extension that is marked executable is not handed to the system to open,
  since that would run it.

### Fixed
- On macOS, files and folders moved or renamed outside TrailMark now show up at once in the tree and in search, and the tags of a renamed
  file follow it, where such a move went unnoticed.
- On macOS, renaming a file or folder in the Explorer so that only the case of its name changes (`readme.md` to `README.md`) works.
- In Simple mode, a folder created next to the open file no longer reports the files inside it as new.

### Changed
- In Spanish the Source mode is called **Código**, the usual short name, so the mode switch stays compact.
- Semantic search is now planned for v0.5 (the disabled **Semantic** toggle in Search says so); v0.4 brings an optional, local
  connection between TrailMark and your coding agent. See the roadmap.

## [0.2.1] - 2026-10-14

TrailMark now speaks Português (Brasil), Español and Deutsch, opens from the Windows file manager, and gets a single
Read | Visual | Source switch, a window that remembers its place, and an Explorer that handles workspaces of several repositories.

### Added
- **One mode switch in the header: Read | Visual | Source.** It replaces the **Edit** button and the Visual / Source switch that only showed while
  editing, and it is always there while a document is open (Simple mode too). One click goes to any mode, keeping your place (the block at the
  top of the view stays at the top, in all three) and your unsaved work; leaving an editor for Read with unsaved changes asks, as before, to
  Save, Discard or Keep editing. `Ctrl+E` and `E` open the editor you used last (the **Open files for editing in** setting until you use one),
  `Ctrl+Alt+E` toggles Visual and Source, or from Read opens the editor you did not use last. For a file that cannot be edited the Visual and Source
  segments are dimmed, with the reason as their tooltip. In a narrow header the labels become icons, and the switch works with the arrow keys,
  Home and End, with tooltips that name the shortcut.
- **Repositories inside a project are always in the tree.** A folder with its own `.git` shows up even when the project's `.gitignore`
  leaves it out, which is how a workspace folder that collects several independent repositories is usually set up. Inside each one its own
  `.gitignore` and `.git/info/exclude` decide what is listed, not the project's. Search, Quick open, tags and live updates cover their files
  too. Linked worktrees work as before.
- **Two new switches in the Explorer filter**, saved with it and off by default: **Show hidden files and folders** (names that start with a dot) and
  **Show files ignored by git**. Folders with more than 2,000 entries that git ignores (`node_modules`, `target`, build output) are listed
  closed, with a "large folder" tag, instead of being read.
- **`.trailmark/` is easier to find.** A quiet banner offers to keep tags and settings with the project when you open a git repository that
  doesn't have a `.trailmark/` folder yet (**Initialize**, **Not now**, **Never for this folder**), and the tag editor has an **Initialize .trailmark**
  link next to "Saved in TrailMark, not in the file". Both open the existing Initialize dialog.
- **The window remembers its size and position.** TrailMark opens where you left it, including maximized, with no jump: the window is
  placed before it is shown. Each project folder keeps its own place (in the project's private state, never in a file you commit), and
  the last window is used for the welcome screen, a folder opened for the first time and a single file. If the screen it was on is gone,
  or the resolution or display scale changed, the window comes back on the main screen at a size that fits.
- **Translations.** The interface now comes in **Português (Brasil)**, **Español** and **Deutsch** besides English. **Settings → Language** offers
  *System default* (the first language of your system that TrailMark speaks, else English) and each language written in itself; the change
  applies at once, without a restart, and is kept for all your projects. Plurals, numbers, dates and relative times follow the language,
  key names in tooltips follow the keyboard of the language (`Strg+Umschalt+O` in German), and the native dialogs, errors and document
  notes are worded in it too.
- **Windows installer languages.** The installer asks for English, Português (Brasil), Español or Deutsch, and TrailMark starts in that language
  until you pick one in Settings. A language chosen in the app always wins, and an update never changes it.
- A test fails when a translation misses a key, has an extra one, changes a placeholder, or leaves a long sentence in English. See
  `docs/design/i18n.md` for the conventions and how to add a language.
- **"Open with TrailMark" in the Windows file manager.** Right-click a folder, the empty space inside a folder, or a `.md` / `.markdown` file and
  choose **Open with TrailMark** (on Windows 11 it is under *Show more options*). A second, separate option lists TrailMark in the **Open with**
  menu of Markdown files; it never takes over your default app (Windows keeps that for you), and **Settings → File manager → Make default...**
  opens the Windows page where you pick it. Both options are in the installer (on by default for a new install, on its first page) and in
  **Settings → File manager**, which adds or removes the entries at once and always shows what is registered now. The text of the entry follows the
  language of the interface. Nothing needs administrator rights. An update keeps what you chose (it never adds back what you removed, nor
  removes what you kept), and uninstalling removes every entry.

### Changed
- Hidden files and folders (`.github`, `.claude`, `.vscode`, `.gitignore`, ...) are no longer listed by default, in any preset. Turn on **Show
  hidden files and folders** to see them; a hidden file that is open still shows, dimmed, in its folder. A hidden folder hides its images too, even
  ones a document links to.
- The **Everything** preset says that files ignored by git (including those in `.git/info/exclude`, such as a local-only script) have their own switch.
- **The Explorer filter keeps your documentation images.** With **Only assets next to documents** on, an image used to show only when its
  folder, or the one above it, held a document, so the pictures of a top-level `assets/` or `site/assets/` folder were hidden. Now an image
  also shows when a Markdown document links to it (`![](...)`, a reference definition, or an HTML `<img>`, resolved from that document, wherever the
  image is), or when it sits in a folder named `assets`, `images`, `img`, `media`, `attachments`, `screenshots`, `figures`, `diagrams` or
  `pictures`. The icons of a code folder stay out unless a document uses them. Data and XML files keep the old rule, and `+pattern` / `-pattern`
  entries still win. The set of linked images is kept up to date as documents change, reading only the document that changed.
- Errors from the backend carry a kind and the name or text they are about, and the interface words them; a few messages that used to be fixed
  English sentences (a file that is not UTF-8, a document shown as plain text because of its nesting) are now worded by the interface.
- **Enter works like a spreadsheet inside a table in the Visual editor.** A table cell can't hold a line break, and Enter used to leave the table.
  Now **Enter** moves to the same column one row down (and adds a row from the last row), **Shift+Enter** moves one row up, and **Ctrl+Enter**
  (**Cmd+Enter** on macOS) leaves the table. **Tab** and **Shift+Tab** still move between cells, and Tab in the last cell adds a row. Undo
  takes an added row away in one step. Enter outside a table is unchanged.
- **The Settings panel scrolls** when the window is shorter than it is (it grew with the new File manager section), and a popover whose
  content changes size after it opened is placed again.

### Fixed
- The folded **More formatting** menu of the Visual toolbar (in a narrow window) now has the table actions (add and delete rows and
  columns), with the reason as a tooltip when one can't be used.
- Opening a drive root from the file manager (`"C:\"` on the command line) works.
- The welcome screen opens at the top with a long list of recent folders, instead of scrolled down past the logo.
- **Quick open** opens a file the Explorer filter hides with one Enter when it is the only kind of match.
- The file name in the header stays readable while editing in longer languages: the "saved ... ago" note and the folders shrink
  first, and **Copy reference** becomes an icon in a narrow window.

## [0.2.0] - 2026-10-14

TrailMark grows from a reader into a Markdown IDE: organize, edit and reorganize the documents you read, with the same promise as
before that a file written by an agent is never rewritten behind your back.

### Added

**Editing**
- **Two editors.** Edit a Markdown document with `E`, `Ctrl+E` or the Edit button. **Visual** is a WYSIWYG editor drawn with the reading styles: a
  toolbar (block type, bold, italic, strikethrough, inline code, links, lists, task lists, quotes, tables, code blocks with a language menu, images and
  rules), a `/` block menu, a link popover that flags a link to a file that doesn't exist, and clickable task boxes. **Source** is a Markdown editor with
  highlighting, line numbers and soft wrap at the reading width. `Ctrl+Alt+E` or the Visual / Source switch in the header moves between them, keeping your
  unsaved work and the block you are in. A setting chooses which one opens first. Both are loaded only when you start editing.
- **Only the blocks you changed are rewritten.** Every other byte of the file stays as it was, including line endings, a byte-order mark and the blank lines
  between blocks, and a save without an edit writes the very same file. Edited blocks carry a bar in the margin until they are saved, and the status bar
  counts them. Blocks the visual editor can't write back as the same Markdown (front matter, raw HTML, math, diagrams, footnotes, alerts, and blocks
  that use a link reference or a footnote) are shown as they read and can't be edited there; **Edit in Source** opens the Markdown at that block.
- **Sharing a file with agents is safe.** If something else changes the file while you edit, a clean buffer reloads in place, and a buffer with your edits shows
  a banner: **Reload theirs** or **Keep mine** (a file deleted meanwhile can be saved again or discarded). The file is compared once more right before a
  save replaces it, so a save never overwrites a version you haven't seen; a save that fails (read-only file, access denied) keeps your text and offers
  Retry and Copy all. Your own saves are not news: no glow, toast, feed entry or Follow jump for what you just wrote.
- **Unsaved work is never lost quietly.** An `Unsaved` mark in the header, a dot on the file in the tree and a `*` in the window title; leaving the document
  (opening another file, closing the window, **Restart to update**) asks to Save, Discard or Keep editing.
- **New file** (`Ctrl+N`): type a name in the tree (`.md` is added), and the file opens for editing with the caret after its title. If the open
  document has unsaved edits, TrailMark asks about them first and creates nothing until you answer.
- **Images** pasted, dropped or picked are saved next to the document in `assets/` and linked with a relative path. Markdown pasted as text becomes
  blocks. Files that are binary, over 5 MB, not Markdown or not UTF-8 can't be edited (the Edit button says why).
- `Ctrl+B`, `Ctrl+I`, `Ctrl+K`, `` Ctrl+` `` and `Ctrl+Shift+X` format the selection in both editors; `Ctrl+Alt+1` to `4` and `0` set the block type in Visual.

**File operations in the Explorer**
- **Rename** (`F2`, in place), **move** (drag and drop, or `Ctrl+X` and `Ctrl+V`), **delete** (`Del`, to the system trash, after asking how many) and **New
  folder**, from the keyboard, the context menu and the `[:]` menu. `Ctrl+click` and `Shift+click` (or `Shift+Arrow`) select several rows. Nothing is
  overwritten, and nothing leaves the project or moves between the project and a worktree.
- **Links keep working.** Moving or renaming a file or folder updates the links and images that point at it, and the relative links inside the documents
  that move: only the destination changes (anchors, queries, titles, percent-encoding and reference definitions are kept, and `href` and `src` in HTML are
  followed) and code is never touched. "Update 7 links in 4 files?" lists every change first: **Apply**, **Move without updating** or **Cancel**; Settings
  has **Update links when moving files: Ask, Always or Never**. A document that changed on disk since the preview is skipped and reported. A document open
  with unsaved edits in Source gets the change in the editor; one with unsaved edits in Visual is left alone and listed as not updated.
- **Undo** with `Ctrl+Z` in the Explorer, or the toast's Undo button, puts back the moves and every rewritten link in the files that are unchanged since, and says
  what couldn't be put back. The live feed shows the operation as one row ("Moved 2 files, 7 links updated").

**Tags and descriptions**
- Tag any file and give it a one-line description. Tags written in a document's frontmatter (`tags:`) are read automatically and shown with a file glyph;
  tags added in the app never touch your files: they are kept in `.trailmark/meta.toml` once the project is initialized (committable, so teammates and agents
  can read it) or in the app's own storage before that. Renamed and moved files keep their tags, and entries of deleted files are kept until you choose "Clean up".
- A strip at the top of the document shows the description and the tags; `T` edits them, with autocomplete. A **Tags view** (`Ctrl+Shift+T`) lists every tag
  with its number of files; pick one, or `Ctrl+click` several, to list the files that have all of them.
- `tag:name` filters work in Quick open and in Search; descriptions show in the tree tooltip, Quick open and search results. A `meta.toml` that can't be read
  (a merge conflict, say) makes the app's tags read-only and says which line is wrong.

**Outline**
- The right panel has two tabs, **Changes** and **Outline**. The Outline lists the open document's headings, indented by nesting; the section you're
  reading is marked and the list follows you as you scroll. Click or press `Enter` on a heading to jump to it; `Left` and `Right` collapse and expand a section.
  The collapsed state is kept per document while the app is open, even when the document is rewritten under you. Long documents show the first 1,000 headings.
- While an agent rewrites the document, a dot marks the sections whose blocks changed. While you edit, the Outline is rebuilt from your text a moment after you
  stop typing, jumps to a heading in either editor and follows the editor's scroll position.
- `Ctrl+Alt+O` shows the Outline (opening the panel if it is collapsed); press it again to go back. In Simple mode it opens as a popover under the header.

**Explorer**
- A **filter** in the Explorer header chooses what the tree shows: Documents, Docs and assets (the default), Everything, or Custom (file-type checkboxes plus
  optional `+pattern` / `-pattern` globs such as `+*.sql -archive/**`). Folders with no visible file are hidden, so a code repository shows `docs/` and the
  root documents instead of every source folder, and JSON, TOML, YAML, XML and image files of code folders stay out unless a document sits next to them.
  The footer says how many files are hidden ("Showing 52 of 1,204 files") and offers **Show all** for the moment; a hidden file you open stays in the tree,
  dimmed, while it is open. Quick open follows the filter and ends with "+N hidden files match". Your choice is remembered per project, and a project can set
  the default for everyone in `.trailmark/config.toml`.
- The Explorer **follows the open file**: whatever opens a document (Quick open, search, a link, the feed, Follow, back and forward, the restored session), it opens the
  folders above it, selects the file and scrolls it into view without taking the keyboard focus. Worktree files appear under their worktree. **Always select the
  opened file** (Explorer `[:]` menu) turns it off; "Reveal open doc" still works.

**Reading**
- **Three reading widths:** Comfortable (760 px, the default), Wide (1100 px) and Full (the whole pane). One click on the status bar, the menu entry or `Ctrl+Alt+W` moves to
  the next; Settings has all three. Tables, code blocks, Mermaid diagrams, display formulas and wide images take the room they need, up to the pane, while
  the text stays at the comfortable measure.

### Changed
- The right panel has a view switch instead of the old "Live changes" title: **Changes** is the live feed and "Since last visit" as before, **Outline** is new, and the tab you
  were on is remembered per project.
- The Explorer shows **Docs and assets** by default, so files that aren't documents may now be hidden. Pick **Everything** in the filter to see all of them again.
- Reading width has three settings instead of two; a project that used the old "Wide" keeps it.
- A document's `tags:` frontmatter now appears as tag chips in the strip above the document instead of in the metadata card.
- "Open in external editor" moved to the `...` menu.
- A new logo and app icon: a bookmark that keeps your place and watches what changes.
- For contributors: the Rust toolchain is pinned in `rust-toolchain.toml`, so local runs and CI share one compiler and one set of lints.

### Fixed
- A project's saved state (last document, panel layout, filters) is now shared by every way of writing its path (short Windows names, `..`), where it was kept
  separately for each.
- The window can no longer be navigated away from the app by a plain link in a document, which would have replaced the app, and any unsaved edits, with the page.
- Settings, session and tag files are flushed to disk before they replace the old ones, so a power loss can't leave an empty file, and a replace that fails
  because another program is holding the file is retried.
- The disabled **Semantic** toggle in Search named the wrong release; semantic search is planned for v0.3.

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
