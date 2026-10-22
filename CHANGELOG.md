# Changelog

All notable changes to PerMark (formerly TrailMark) are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added
- **Projects inside a folder.** When a folder you open holds other projects (folders with their own git repository or their own PerMark settings), a quiet note under the file list says so once and offers to include them, to choose, or No thanks; a short message with Undo follows your answer and then closes by itself. Included projects are followed from that folder: their changes and unread documents show there, and the file list marks each one with a project icon next to its folder name (and its short name when that is different). Resting the pointer on it, or choosing Project details in its menu, shows a small card about it. The new **Settings > Projects inside** lists every project found, included or not, with a filter, buttons to include or leave out all the projects listed, and for each included project its details and rules: hidden files, the agent, notifications, and its name and description. Projects that went missing can be located again or removed from the list, with Undo. When new projects appear, the note says where they are; when the folder's project list can't be read, the note says so and shows you the problem. The Changes panel says which project a change belongs to and lists projects that appear, move or go away (a project that went missing opens its place in Settings), and the Search panel can look in the whole folder or only in the project of the open document, and its search box says which. Before you answer, the note and the Choose page say that the list is saved in the folder (`.permark/projects.toml`), so it travels with it; a project's name and description are saved in that project's own `.permark/project.toml`, as the fields say. A folder without projects looks exactly as before. <!-- f: nested-projects -->
- **Git for each project inside a folder.** An included project with its own git repository now keeps its own git status in the folder that includes it: its changed files belong to that project, no longer to the outer folder, and comparing one of its documents with "the last commit" uses the project's own repository. PerMark reads each project's branch from its files at once and asks git about changes only when they matter (the project's files change, or you look at the project), a few projects at a time, so a folder with dozens of projects stays fast. PerMark also notes each project's first commit in the folder's project list, so a project you moved or renamed while PerMark was closed is found again the next time you open the folder: PerMark asks you first, naming the folder it found ("Is tools/new-app the project hub-x that was at hubs/hub-x?"), because copies of a repository look the same to it, then says what it did and puts you on that folder in the file list. A copy you said no to is never offered as the moved project. In the file list, each such project shows its branch in gray after its name, with a small branch mark so it reads apart from the project's own short name, also before the project's first commit (or what git is doing there, such as REBASING, which is never cut short, or "not on a branch" with the commit), a warning mark when git can't read the project, and an open project marks its changed files as the outer folder's files are marked; in a narrow sidebar the branch steps aside. Its card adds the branch, the changes to send and to get as of the last fetch (↑ ↓), how many files changed and its last commit ("No commits yet" in a new repository), and a screen reader hears these when the card opens. Wherever a label such as MERGING or REBASING appears, its tooltip and screen readers say what it means in plain words, such as "A merge was started in git and isn't finished yet." Settings > Projects inside shows the branch and the number of changed files on each row ("Checking git…" while PerMark is still asking), and the status bar follows the project of the document you are reading, with the project's short name before its branch; choosing "Changes since: the last commit" there applies to that project's files too. When git is not available, no branch is shown anywhere and Settings says so once. <!-- f: nested-projects, git-status -->
- The file list's menu now also opens from the keyboard, with `Shift+F10` or the Menu key, at the row you are on. <!-- f: explorer, keyboard-shortcuts -->

### Changed
- When a folder you share with an agent holds other projects, the agent no longer sees the files inside them until you include those projects (see **Projects inside a folder** above); this holds for projects in visible folders too. For a project you include, the private files of its own `.aiignore` and `[agent]` settings are hidden from the agent as well as the folder's, and you can turn the agent off for that project alone. A project you open on its own keeps the files its outer folder hides from agents hidden. A project you said no to stays left out, even if the folder's project list names it later (the Changes panel tells you, and you can include it in Settings). While PerMark is still reading a folder it has just opened, the agent waits a moment instead of seeing it half read. <!-- f: nested-projects, agent-connection -->
- The **Project** section of the settings is now called **This folder**, so it reads apart from **Projects inside** in folders that hold other projects. <!-- f: nested-projects, settings -->

### Fixed
- In the git pop-up of the status bar, the arrow keys now move between the two "Changes since" choices, Escape takes you back to the git item instead of losing your place, and a label such as MERGING is easier to read in the light theme. <!-- f: git-status, keyboard-shortcuts -->
- `Esc` now closes the review of changes, as the Done button's tooltip says. It still closes a menu, a dialog or a text field first when one of them is open. <!-- f: review-changes, keyboard-shortcuts -->
- In a folder without git, "Compare with: the last commit" in the review is now shown as unavailable, and choosing it says why next to it. Before, it seemed to switch but kept showing the same changes. <!-- f: review-changes, git-status -->
- When you open the review from its bar with the keyboard, your keys now go to the review's own buttons; when you close it, they go back to the page, which shows a focus frame. Before, they were lost and the next `Tab` skipped the review's buttons. <!-- f: review-changes -->
- The `Ctrl+Alt+E` command is now called "Open Source mode" in the command list and the keyboard shortcuts, which is what it does. It was called "Switch between Edit and Source". <!-- f: source-mode, keyboard-shortcuts -->
- Screen readers now read each checklist box in a document together with its task, not only "checkbox, checked". <!-- f: view-mode -->
- After you drop files onto the file list, the question that asks Copy here or Move here now takes the keyboard, so `Enter` copies at once. <!-- f: drop-files -->
- When you switch to another tab while the question about files you dropped on the file list is open, the question now waits in its own tab until you answer it, also when you click another tab. Before, it closed without a word and nothing was copied. If the question can no longer be answered, for example because another project opened in that tab, a notice says that nothing was added. <!-- f: drop-files, tabs -->
- Small gray text is easier to read in the light and dark themes: the "EXPLORER" heading, the time a document was saved, the names of a document's properties, the outline heading and the app name. <!-- f: themes, explorer, outline, frontmatter-card -->
- When a folder you open holds other projects in folders that its own `.gitignore` leaves out (for example a workspace that ignores `projects/`, where each project has its own git repository), PerMark now follows what changes inside those projects as it happens: the Changes list, the highlights of changed passages, follow mode, unread markers, search and the file list stay current, and open documents reload. Projects you add, remove or rename while the folder is open now appear or go away without opening the folder again. Before, everything inside them stayed as it was when the folder was opened. <!-- f: live-feed, explorer, unread, search -->
- A project you included keeps its place when you rename its folder or move it to another folder inside the opened folder while PerMark is open, also in Windows File Explorer: it stays included under its new name, and the Changes panel shows one "moved" line. Before, PerMark took it for a project that went away and a new one that appeared, and offered to include it again. This also holds when the system reports the move in two parts, as macOS sometimes does: PerMark waits about two seconds before it says that a project went away, so a move never shows as a project that went away and a new one, not even for a moment. When you change only the capital letters of its name, the project list follows the new spelling. Locate… for a project that went missing now opens the folder picker in the folder where the project was, or in the opened folder when that one is gone too, instead of the last folder the system picker showed. <!-- f: nested-projects -->
- Closing a tab or a window, or opening another folder in it, no longer gets stuck when PerMark is still reading a large folder for search. Before, that could happen now and then, and the folder was never let go. <!-- f: search, tabs -->
- When an agent names itself in a request, PerMark now always shows and records that name. Before, the name it gave when it connected could sometimes replace it a moment later. <!-- f: agent-connection -->

## [0.6.3] - 2026-10-21

### Fixed
- Right after a project opens, an agent's search now finds the keys of configuration files too; before, it could miss them for a moment while saying the answer was complete. <!-- f: agent-connection, search, config-sheets -->
- When two projects copy files in at the same time, each progress card now shows only its own project's copy; before, the cards could mix both. <!-- f: drop-files, tabs -->
- A document that changes in a tab you are not looking at stays unread, and its changes stay offered for review, until you go to that tab and read it. Before, it could be marked as read while it was hidden behind another tab. <!-- f: unread, review-changes, tabs -->
- When you switch tabs while PerMark is still preparing the question for files you dropped, your keys stay in the tab you switched to; the question waits in its own tab. <!-- f: drop-files, tabs -->
- A Notes tab behind another tab no longer checks for new notes every few seconds; it looks again when you come back to it. <!-- f: notes, tabs -->

### Known limits
- Tabs, moving a tab to another window and dropping files have not been tested on macOS yet. There, the arrow keys in text fields may not work as expected; a fix is planned. <!-- f: tabs, windows, drop-files -->

## [0.6.2] - 2026-10-20

### Added
- **Notes button.** A button next to Settings opens your Notes in a tab with one click, or brings you to the Notes tab if it is already open. `Ctrl+Shift+D` does the same, and `Ctrl+Shift+N` makes a new note from anywhere. <!-- f: notes, keyboard-shortcuts -->
- **Drop files from outside, anywhere.** Drag files or folders from your file manager onto the file list and the folder under the pointer lights up; when you let go, a small question at that spot asks Copy here, Move here or Cancel, and lists names that already exist (Keep both or Skip). Drop them anywhere else in the window, the start screen included, and each one opens as a tab; more than 8 ask first. Text files dropped on the Notes tab (Markdown, plain text, settings and data files such as YAML, JSON or CSV, and files like README) become loose files there, and pictures dropped on a document you are editing go into it. A label next to the pointer always says what the drop will do, in place of the dashed frame that covered the window, and folders you rest on open by themselves. Undo in the notice takes back exactly that drop, even after other changes, as long as those files were not touched since. <!-- f: drop-files, explorer, tabs, notes, start-screen, pictures-in-documents -->
- **Hide and show the side bars.** `Ctrl+B` hides or shows the sidebar and `Ctrl+Alt+B` the right panel, as in earlier versions; a button at each end of the status bar does the same. Each project remembers which side bars you hid. <!-- f: side-panels, keyboard-shortcuts -->
- **Move a tab to another window.** Right-click a tab and choose **Move to window** to move it into another open window; the list shows each window by the tab it is showing and how many tabs it has. Unsaved changes move with the tab. Moving a window's last tab closes the window it leaves. <!-- f: tabs, windows -->

### Changed
- Every plugin is now on from the start, Token count and the Converter included, so you no longer need to turn them on in the Plugins page. A plugin you turned off yourself stays off. Token count still asks before it downloads a tokenizer's vocabulary, and nothing is downloaded when PerMark starts. The agent connection stays off until you turn it on for a project. <!-- f: plugins, token-count, converter -->
- `Ctrl+B` makes text bold in the source editor too, as it already did in the visual editor. <!-- f: source-mode, keyboard-shortcuts -->
- The outline button always shows the outline, even when another panel was open on the right. <!-- f: outline, side-panels -->
- In the Notes tab, `Ctrl+1` to `Ctrl+9` open your pinned notes (instead of switching tabs), `Ctrl+N` makes a new note and `Ctrl+Shift+C` copies the note. Use `Ctrl+Tab` or the tabs to switch tabs from there. The keyboard shortcuts list no longer reports these keys as shared, but it does warn when a key you choose is already used where the same command could run. <!-- f: notes, keyboard-shortcuts -->

### Fixed
- Clicking a note on the start screen, or "New note", now reaches the Notes tab even when it is open in another window. <!-- f: notes, start-screen -->
- When another program saves `.permark/meta.toml` at the same moment PerMark writes your tags or descriptions there, PerMark no longer writes over that program's version: it reads it first and then adds your change. <!-- f: tags -->
- Files you add, copy or move in a project now appear in its file list right away, also while another open project is changing; before, they could stay out of the list until it was reloaded. <!-- f: explorer, tabs -->

### Known limits
- Tabs, moving a tab to another window and dropping files have not been tested on macOS yet. There, the arrow keys in text fields may not work as expected; a fix is planned. <!-- f: tabs, windows, drop-files -->

## [0.6.1] - 2026-10-20

### Added
- **Tabs.** Each folder or file you open gets a tab in the bar at the top of its window, and Notes open in a tab too. Click a tab to switch to it; `Ctrl+Tab` and `Ctrl+Shift+Tab` (or `Ctrl+Alt+→` and `Ctrl+Alt+←`) go to the next and previous tab, `Ctrl+1` to `Ctrl+8` to a tab, `Ctrl+9` to the last one, and `Ctrl+W` closes the tab. A tab with unsaved changes shows a dot. Right-click a tab to move it to a new window. When PerMark starts, your windows and tabs come back, and only the tab you see loads, so the start stays fast. <!-- f: tabs, notes, keyboard-shortcuts, session-restore -->

### Changed
- A folder or file opens in one place only: if it is open already, its window and tab come forward. **Settings → Editing → Open folders and files in** offers New tab (the default), This tab and New window; the old "Ask" choice now means New tab, and the question dialog is gone. <!-- f: tabs, settings -->
- Opening from outside PerMark (a double-click, the command line) adds a tab to the window you used last. <!-- f: tabs, file-manager-integration -->
- Closing a window with unsaved changes asks once for all its tabs: Save all, Discard or Cancel. <!-- f: tabs, save -->
- The windows of 0.6.0 come back as tabs: each window keeps its folder or file as one tab, and a project that was open in two windows comes back once. <!-- f: tabs, session-restore -->

### Fixed
- Windows come back where you left them, also when PerMark starts from a file you open. A new window opens slightly offset from the current one, at the same size, and a maximized window stays maximized. The first start after 0.5 takes the window place 0.5 saved. <!-- f: windows, session-restore -->
- The document header no longer covers the outline or the live feed in a narrow window. When space runs short, less used buttons move into the "…" menu, and in a very narrow window the mode buttons show as icons. <!-- f: view-mode, outline, live-feed -->
- The scrollbar of a document is always visible and wider, so it is easier to grab. <!-- f: view-mode -->
- The buttons of the side bar (such as Broken links and Tags) always appear in the same order. <!-- f: side-panels -->

### Known limits
- Tabs have not been tested on macOS yet. There, the arrow keys in text fields may not work as expected; a fix is planned. <!-- f: tabs -->

## [0.6.0] - 2026-10-20

PerMark is now a unified document workspace: one place to read, write and organize Markdown and the files around it, and to steer AI projects through their documents. AI speeds the work. Your vision steers it. This release rebuilds the whole program: many windows in one app, small config files and TOON as clean sheets, review of code and text files, and an agent connection with 13 tools. The name, the logo, your files and your notes stay where they were.

### Upgrading from 0.5
PerMark 0.6 never changes your files to upgrade. It reads what 0.5 saved and copies it once; 0.5's own data stays where it was.
- **Carried over:** your notes folder and how notes are named, with the notes you pinned or shared and the files you opened in Notes; tags and descriptions; comments, including the ones you marked done; the folders you silenced; and your language and external editor. Projects that have a `.permark/` folder keep their tags, settings and comments in it, as before.
- **Not carried over:** the text you typed into a file opened in Notes and had not saved (it stays on disk in the notes folder's `.permark/pending/`, untouched), which documents you had read or reviewed (every document starts as read), the recent folders list, window places, the earlier versions of your notes (new versions start now), the on-off switch of the agent connection (it starts off in every project: turn it on again where you want it), and the rest of the app settings (look at **Settings** once). Your files are never touched.
- **Setting up again:** the file a script can read for the open document is now `.permark/local/agent-context.json` (it was `agent.json`).
- **Old TrailMark folders:** 0.6 no longer renames `.trailmark/` folders. If you still have TrailMark, update to 0.5.8 first.
- **Going back:** 0.5.8 cannot read the comments file after you mark a comment done in 0.6.

### Added
- **Many windows, one app.** A bar at the top lists the open projects, files and Notes, switches between them (`Ctrl+Alt+→` and `Ctrl+Alt+←`) and closes them with the x. The **+** menu opens a folder or a file, starts a new project, opens Notes or picks a recent folder.
- **A full Settings page, a Plugins page and your own keys.** Turn features on and off in **Settings → Plugins**; a feature that is off leaves nothing behind. **Settings → Keyboard shortcuts** shows every command, lets you change its key and warns when two commands share one.
- **Small config files as clean sheets.** TOML, YAML, INI, `.env`, `.properties` and ignore files (`.gitignore`, `.aiignore` and the like) open as sections, keys and values, with comments as captions. Editing a value changes only those bytes: comments, order and spacing stay. A part that cannot be edited exactly stays read-only, and Source is always there. Secret values (keys, tokens, passwords) are masked in the sheet.
- **TOON.** `.toon` files, the compact data format made for AI, open as tables and cards and can be edited in place. A `toon` block inside Markdown draws the same way.
- **Review code and plain text.** What an agent changed in a code file, a log or a data file shows line by line, and you keep or revert each change, as you do in Markdown. Lines that change while you watch glow. Very large text files open at once, the first part first.
- **Next and previous change.** `F7` and `Shift+F7` go to the next or previous change of the open document: the changes you have not reviewed yet, or the passages that are glowing.
- **Focus mode** (`Ctrl+Alt+F`) leaves only the page.
- **Drop files into a project.** Files and folders dragged in from the file manager come into the project, into the folder under the pointer. You choose copy or move, names that already exist are listed first, and one Undo takes it all back.
- **Source is the raw file.** Every byte as it is: nothing hidden, masked or drawn over it. It has find and replace (`Ctrl+F`, `Ctrl+H`) and go to line (`Ctrl+G`). A switch beside the modes shows a Mermaid or PlantUML drawing next to the text while you type (off until you turn it on).
- **Web pictures on request.** A picture from a web address shows as a chip with its description; a click loads that picture. In the document menu, **Load web pictures in this project** loads them by themselves for that project.
- **Token count** (off until you turn it on): how many tokens a document, a section or a folder takes. Exact for OpenAI's tokenizers (it asks before it downloads the small vocabulary), marked as approximate for Claude and Gemini.
- **Converter** (off until you turn it on): turns JSON, XML, YAML, CSV and TOML into TOON or another compact format (or into a readable one), shows the estimated token saving of each choice, and never writes over the original.
- **Markdown of the AI era is recognized** with no setup: agent files such as `AGENTS.md`, `CLAUDE.md` and `SKILL.md`, Cursor rules (`.mdc`), `llms.txt`, prompt files, and model and dataset cards (told by their front matter).
- **A bigger agent connection.** It now has 13 tools, with short answers an agent spends few tokens on. New: `map` (the project's files with sizes, tags and descriptions), `outline` (the parts of a file before reading it, optionally with the links in and out), `read` (one part of a file), and `activity` (what you did since the last hand-off). `search` takes a token budget. Permissions are set by group, per project, and a project's `.permark/` can only narrow them. Files in `.aiignore` and private key files never reach an agent, and secret values are always masked. **Hand off to agent** copies one line for your prompt so the agent asks what you did instead of rereading the project. An agent still only reads, shows you a place and marks comments done; it never edits a file through PerMark.
- **Prompt blocks.** XML-like prompt tags such as `<context>`, `<instructions>` and `<example>` fold as blocks, and the Markdown inside them still renders; `{{name}}` template variables are marked and `@path` references link to the file. <!-- f: prompt-blocks -->
- **Word count** in the status bar: words, characters and reading time of the document or of the selection. <!-- f: word-count -->
- **Front matter you can edit.** In Edit, the front matter shows as properties you can change without breaking the file; in View it stays a read-only card. <!-- f: frontmatter-card -->
- **List indent and outdent** buttons in the Edit toolbar.
- **Choose a picture.** The picture button in Edit opens the system file dialog; the pictures are saved next to the document.
- **The interface speaks six languages**: English, Português (Brasil), Español, Deutsch, 简体中文 and Italiano.

### Changed
- **One program for every window.** Opening a second project no longer starts a second copy of PerMark.
- **View, Edit and Source.** The header switch is View for the rendered document, Edit for changing it, and Source for the file's text. The shortcuts stay `Ctrl+E` and `Ctrl+Alt+E`.
- **Delete in the Explorer** goes to the system trash at once, without asking. `Ctrl+Z` brings it back (on macOS the app cannot take a file out of the Trash).
- **Heading levels are easier to tell apart**, in reading and in Edit, and heading shortcuts (`Ctrl+Shift+1` to `6`) work on every keyboard layout.
- **Git.** PerMark uses the git installed on your computer, or its own built-in git when there is none. It still only reads git: it never commits, stages or creates a repository.
- **Choosing PerMark for Markdown files** is your decision: the installer asks (Markdown types checked, the others not), and on Windows it takes you to the Windows page where you pick the default app.
- **Quick open** searches all files that are not ignored; hidden ones (names that start with a dot) come after the others unless the Explorer shows them. The tag filter moved to the **Tags** scope of Search everywhere.
- **"Since your last visit"** compares the content of your documents, not file dates, and works without git.
- **Follow mode** waits while you are editing.
- **Notes** keep the 0.5 folder and file names. The newest 50 earlier versions of a note are kept, and a note you were typing is recovered after a crash.
- **Updates** check once a day, and a restart to update is refused while you have unsaved files, with a list of them.
- **Code blocks** in Edit take the language as text. Descriptions can be 160 characters.
- **Pictures** you paste or drop are saved next to the document.

### Removed
- The setting that chose between reloading a changed file by itself or asking: a file you have not edited reloads, and one with your edits asks.
- The Explorer's **Show only unread** filter (the **Unread** tab lists the documents), the Outline's popover and its marks for changed sections.
- The table toolbar's **Add row above** and **Add column to the left** (below and to the right remain).
- Follow mode's pause while you scroll.
- The disabled **Semantic** switch in the search panel.
- The conversion of old `.trailmark/` folders (see Upgrading).

### Fixed
- The headings of a document looked alike from level 2 on; each level now has its own look.
- A nested list item could not be moved out a level from the toolbar.

### Known limits
- PerMark 0.6 was tested on Windows. The macOS and Linux versions are built and checked by the project's CI and have not been tested by hand yet.
- The installers are not code-signed, so your system warns you the first time.
- The `.msi` installer does not add the **Open with** entries and its uninstall leaves them; use the `-setup.exe`. The `.msi` stays for updating copies installed from a 0.5 `.msi`.
- A window with a single file does not follow links to other documents.
- Find in page misses a match that runs across two styles (`**bold**` and plain text together).
- Edit mode does not draw pictures that are files next to the document (View and Source do).
- When a file changes while you edit it, the conflict choice is for the whole document, not change by change.
- The search panel does not refresh by itself when files change: search again.
- When live updates stop working (the system ran out of watches), PerMark does not tell you.
- A git worktree added while a project is open, and a `.permark/` folder made outside the app, show up after you reopen the project. Tags of a file renamed while PerMark was closed stay on the old name until **Clean up**.
- Search results have no git or unread badges.
- Config sheets show text over 256 KB or 3,000 rows as plain text, cannot add or remove list items, and show YAML flow style (`{a: 1}`, `[1, 2]`) read-only.
- On Windows with several input languages installed, the system can keep `Ctrl+Shift+0` for itself, so the paragraph shortcut in Edit is `Ctrl+Alt+0` (`Ctrl+Shift+1` to `6` for headings are not affected).
- The agent connection hides secret values by their key names. A secret in the middle of ordinary text, such as a private key pasted into a note, is not recognized, and an agent that searches again and again can guess a short secret from whether a search finds it. List such files in `.aiignore`.
- Reverting a change at the very end of a file whose last line break differs, and restoring a deleted document that used Windows line endings, can leave a different line ending than the original.

## [0.5.8] - 2026-10-18

Draw a PlantUML diagram in the page, start a new file from a folder, and keep a wide block inside the pane.

### Added
- **PlantUML diagrams.** A `plantuml` or `puml` block draws in the reading view, the same way a Mermaid block does. In Visual the source stays, with a live preview beside it. No Java and no PlantUML server. A diagram that would fetch a file does not run.
- **New file.** Right-click a folder, or the project name, and choose **New Markdown file** or **New text file**. The same two entries are in the Projects menu and in the Explorer menu. A text file opens in Source. On Windows, right-click a folder or its background and choose **New Markdown file with PerMark**: the folder opens with `untitled.md` in it.
- **A note's path.** From a note or a loose file, **Copy path** puts its path on the clipboard, and **Show in file manager** selects the file. Copy stays available while the path is known. A file that is missing is not shown.

### Fixed
- **Heading shortcuts in Visual.** `Ctrl+Alt` with a number is the AltGr key on Windows, so those chords typed a character instead of changing the block. Paragraph and headings 1 to 6 are `Ctrl+Shift+0` through `Ctrl+Shift+6`.
- **A wide block stays inside the pane.** Prose wraps in the column. A long table row or a diagram line scrolls inside its block, instead of growing the page past the window.

## [0.5.7] - 2026-10-18

Review what an agent changed, send your comments to it, and keep a noisy folder quiet.

### Added
- **Silence a folder.** Right-click a folder or a file and choose **Silence**. It stays in the tree, and it opens and edits, but its changes, unread marks and Git letters stay quiet. A folder inside it can **Listen** on its own, and that choice covers what is already there and what appears later. **Listen** brings the folder back as already read, so older changes do not all arrive at once. Search leaves silenced paths out until **Include silenced** is on. An agent's search and unread list are unchanged.
- **Complete as you type, without AI.** In Source, a link target offers files in the folder as a path from the current document, and after `#` the headings of that document. A word you have started offers words from the document and from file names. In Visual, the link box does the same for its target, and a word you have started offers those words beside the caret. Enter or Tab accepts; Esc closes.
- **Projects.** A Projects button at the top of the activity bar lists recent folders and offers New project, Open folder and Close folder. New project adds a `.permark` folder to the folder you pick. Opening a folder, a recent folder, a new project, or a folder from the Explorer's menu asks whether it opens in this window or in a new one.
- **Broken links.** A Broken links panel lists every link in the project whose file does not exist. When one file with that name exists somewhere else, Repair points the link at it. When several do, you pick which one.
- **See a Mermaid diagram while you edit its text.** In Source, with the caret in a Mermaid block, the drawing sits beside the text and updates as you type. A text Mermaid cannot draw yet keeps the last drawing of that block and says so. Dragging nodes is not part of this.
- **Review what changed since you last read a document.** A quiet bar names how many blocks changed. Review shows each one in the reading view, and Keep or Revert accepts it or writes the old text back. Your own edits are already approved, and a change is not written if the file moved on while you were looking.
- **Send comments to your agent.** In the Comments tab, **Send to agent** on a comment, or **Send all** at the top, copies a prompt with the document, the section, the lines, the quoted passage and your words. Paste it into any agent chat. The prompt asks the agent to mark each comment done with a short reply. `get_context`, `permark context` and the state file say how many comments are open, so an agent that reads where you are knows to look.

### Changed
- **简体中文 wording** is tighter in a few places (the tagline, a few hints and the Git legend), following a native speaker's review.

## [0.5.6] - 2026-10-17

TrailMark is now PerMark. Same app, same logo, a name of its own.

### Changed
- **New name: PerMark.** The app, its command (`permark`, `permark mcp`, `permark context`), its MCP server and the project folder
  (`.permark/`) take the new name. Nothing is lost on the way: an existing `.trailmark/` folder becomes `.permark/` the first time
  PerMark opens the project, and your settings, recent folders and notes carry over. If an agent is set up with the old
  `trailmark` command, run the setup lines from **Settings → Agent connection** again.

## [0.5.5] - 2026-10-16

Tags get colors and icons and help your agent find the right documents; switch folders, retrace your steps, and open web pages in the browser.

### Added
- **Your agent can find the right documents by their tags.** A new `list_tags` tool shows which tags your documents carry, how many documents
  have each, where each tag comes from (the documents or TrailMark) and how many documents have no tag at all; give it a tag to see the
  documents that have it, each with its description. `search` takes an optional list of tags to look only in the documents that have all of
  them, and every hit now names its document's tags and description; with tags and no words it lists those documents instead. In a project
  of a thousand documents an agent can narrow down in a call or two and read only the few that fit. Answers stay short (long lists show the
  first ones and say how many more there are, and `limit` asks for more), and everything works when TrailMark is closed, from the project's
  files.
- **Give a tag a color and an icon.** Right-click a tag in the Tags view (or press the menu key or Shift+F10 on it) and choose **Color and
  icon...**: pick one of eight colors and one of twelve icons, with a live preview, and the tag looks that way everywhere it shows, so you can
  tell at a glance what kind of document you are looking at: in the strip above a document, in the tag editor and its suggestions, in the
  search boxes and in the Tags view. The colors are tuned to stay readable in the dark theme and in every light tone. A tag with neither looks
  as before. The choice is kept per project, in `.trailmark/meta.toml` with the rest of your tags (`[tag_styles.<tag>]`), so it travels with the
  project.
- **Open web pages in your browser.** An `.html` or `.htm` file in your folder now has **Open in browser**: in its right-click menu in the
  Explorer, in the **...** menu of the open page, and in the command palette. The page opens in the application your system uses for web
  pages, which is your browser on almost every computer.
- **See where you have been, and jump back to it.** Right-click (or press and hold) the **Back** or **Forward** button to list the documents
  on that side, nearest first with the folder beside each name, and pick one to go straight there; Back and Forward carry on from that
  place, like in a browser. **Go to a recent place...** in the command palette lists your whole trail. The menus work from the keyboard too
  (the menu key or Shift+F10 on the focused button).
- **Switch folders without leaving the window.** The project's name at the top of the Explorer is now a menu (the little arrow beside it
  says so). It lists your recent folders, with the open one ticked: pick one to show it in this window, or use **Open folder...** and
  **Close folder**, which takes you back to the start screen. A small **x** beside a folder takes it off the list; only the list forgets
  it, the folder and its files stay where they are. If you have unsaved edits, TrailMark asks to save them first, as it does for any
  other move. The same menu opens from the command palette (**Switch folder...**, **Close folder**), and the project's name stays in
  view while a long file tree scrolls.

## [0.5.4] - 2026-10-16

TrailMark speaks Italian, the light theme comes in softer tones, and the Git letters explain themselves.


### Added
- **Soft light tones.** **Settings → Light tone** adds **Paper** (warm cream), **Sage** (soft green) and **Gray** (soft warm gray) beside the plain
  **White** of the light theme, for readers who find pure white tiring. The tone applies whenever the theme is light (also when *System* picks
  light), in every window (the welcome screen and Notes too), and is kept across projects; the dark theme ignores it. Text, links, code and git
  colours are tuned for each tone so every pair keeps at least the contrast it has on white.
- **The Git letters are explained.** Hover over a badge in the Explorer for what it means in plain words ("M · Modified: this file changed since
  your last visit..."), or open **What do these letters mean?** from the info button in the status bar or from the Explorer menu: it lists M, A, U,
  D, R and C, and says that TrailMark only reads Git and never commits, stages or creates repositories.
- **Italiano (Italian).** The interface now comes in Italian besides English, Português (Brasil), Español, Deutsch and 简体中文.
  **Settings → Language** lists it, and *System default* picks it for any Italian system language (`it-IT`, `it-CH` and the others). The
  Windows installer offers it, "Open with TrailMark" in the file manager is worded in it ("Apri con TrailMark"), and the Microsoft Store
  package declares it. Thanks to @kelvin-mariano for the translation.

### Fixed
- **Buttons look like buttons again** in the Settings, tag and agent panels, and the tag editor's **Done** is readable in the dark theme.
- **A tag that was typed but not yet added is kept** when the tag editor closes (Done, Esc or a click outside); a hint says Enter adds it.
- **Long lists show their scrollbar**: with hundreds of files the thumb no longer shrinks out of sight.
- **Links in the light theme** have more contrast.

### Changed
- **Settings are grouped** into Reading, Editing, Integration and Updates, and long explanations fold behind an info button.
- **简体中文 wording** follows a native speaker's review (批注 for comments, a clearer note / caution pair, 描述, 基准).
- **The Initialize dialog and the hint to keep settings with the project say what they do**: no repository is created and nothing is committed,
  and the lines for `cache/` and `local/` describe what they hold.

## [0.5.3] - 2026-10-16

TrailMark speaks Simplified Chinese, and search finds words in Chinese and Japanese text.

### Added
- **简体中文 (Simplified Chinese).** The interface now comes in Simplified Chinese besides English, Português (Brasil), Español and Deutsch.
  **Settings → Language** lists it, and *System default* picks it for any Chinese system language (Traditional Chinese systems read Simplified
  Chinese for now). The Windows installer offers it, "Open with TrailMark" in the file manager is worded in it ("用 TrailMark 打开"), the Microsoft
  Store package declares it, and the interface and documents use a Chinese font on every system.

### Fixed
- **Search finds Chinese and Japanese words.** These languages are written without spaces, so a sentence used to count as one giant word and
  searching for a word inside it found nothing. Search now matches the characters and character pairs of Chinese, Japanese kanji, hiragana and
  katakana text, in section results and in the agent bridge alike: "搜索" finds it inside a long sentence, a longer query ranks the section that
  holds all of it first, and a single character works too. Mixed text such as `使用Rust编写` is split at the script change. Matches are
  highlighted as one stretch in the result snippets.
- **Asking to delete one file names it** again (in every language), and the toast after deleting a note shows its title.
- **Dates, times and lists are written the way each language writes them**, with no stray comma between a date and a time.
- **Tables stay readable in a narrow pane**: columns keep a sensible width and a wide table scrolls sideways, in Read and in Visual.
- **Typing with an input method** (Chinese, Japanese, Korean): Enter and the arrow keys go to the input method while it is composing,
  instead of creating a file or opening a search result.
- The Notes word count counts each Chinese or Japanese character as a word; Chinese text in code blocks uses a Chinese font; a cut-short
  breadcrumb shows its full name on hover; the footnote back-link is labelled in the interface's language.

## [0.5.2] - 2026-10-16

Pictures, audio and video open in the pane, code blocks are highlighted for about 190 languages, and Notes is simpler to start with.

### Added
- **Code blocks in every mode, for about 190 languages.** A fenced block (`` ```python ``) is now highlighted for every language highlight.js
  ships, loaded one language at a time, so reading does not carry them all. Usual short names work (`js`, `ts`, `tsx`, `py`, `sh`, `zsh`, `ps1`,
  `pwsh`, `yml`, `c#`, `c++`, `kt`, `rb`, `rs`, `golang`, `make`, `jsonc`, `json5`, `svelte`, `vue`...), in any case, and so do the forms docs tools
  write after the backticks: `rust title="main.rs"`, `rust,ignore`, `{python}`, `{.python .numberLines}`, `js:app.js`.
  - **Reading:** each block has its language in the corner, next to the copy button ("Python", "C++", "TOML"). A block that names no language, or one
    that highlight.js does not have, is colored only when the code gives its language away (a `#!` line, JSON that parses, an XML or HTML prolog,
    a diff, a clearly Python script) or when highlight.js's guess is strong and clearly ahead of the runner-up; it is then labelled "auto: Python",
    with a tooltip that says it is a guess. Anything less certain stays plain (directory trees, terminal output, ASCII drawings, prose). `text` and
    `txt` blocks are never colored or guessed at. A name highlight.js lacks (`terraform`) is shown as written, uncolored.
  - **Visual:** code blocks are colored while you edit, with the same colors as in Reading, and carry the language in their corner. The language
    button of the toolbar opens a picker that searches every language by name or alias; text that no language is named by is offered as it is, and
    what you type goes into the fence exactly as typed. Changing the language rewrites only that block's opening fence line (a `~~~` fence or a
    four-backtick fence stays as it was written).
  - **Source:** the code inside a fence is highlighted with CodeMirror's own language support, loaded when a fence asks for it.
- **Pictures open in the pane instead of "looks like a binary file".** PNG, JPEG, GIF (animated), WebP, AVIF, BMP, ICO and SVG, from the Explorer,
  Quick open, search, a link or the feed. A picture is fitted to the window; a click or the button shows it at 100%, `Ctrl+=`, `Ctrl+-` and
  `Ctrl+0` zoom it (and the status bar says how far), a checkerboard sits behind transparency in light and dark, and the header gives the size in
  pixels and on disk. A picture that changes on disk reloads by itself. An SVG is drawn as an `<img>`, never inline, so nothing in it runs.
- **Audio and video play in the pane.** MP3, WAV, OGG, Opus, FLAC, M4A, AAC and WebM audio, and MP4, M4V, WebM, OGV and MOV video, in the native
  player, with the length (and the pixels of a video) in the header. Files are served in parts to the player's `Range` requests, at most 4 MiB
  at a time, so a long file starts at once, seeks, and is never read whole. A format or codec the window cannot play says so, with **Open with
  default app**.
- **`<video>` and `<audio>` in documents, and links to media.** Players in a document play in place, with their files resolved next to the
  document (also in the Visual editor's read-only blocks and in a loose file's folder). A link to an `.mp4`, `.wav` and so on opens in the pane.
  `autoplay`, event handlers and inline data are dropped, a player always has controls and reads only the length of its file until you play it,
  and `poster` files follow a move like any other link.
- **Open with default app.** Any file of the project can go to the program the system has for it: a button on the pane of a file TrailMark cannot
  show, an item in the Explorer's context menu (for every file but a Markdown document), an item in the header's menu, and a command. It is separate
  from **Open in external editor**, which uses your editor command. Programs and scripts (`.exe`, `.msi`, `.bat`, `.cmd`, `.ps1`, `.vbs`, `.js`,
  `.lnk`, `.url`, `.sh`, `.app`, `.pkg`, `.deb`, `.rpm`, `.appimage`, `.website`, `.rdp`, Access databases, macro documents and the older Office
  formats that can hold macros (`.doc`, `.xls`, `.ppt`), every type of `%PATHEXT%` on Windows, anything with an execute permission on macOS and Linux
  and the like) are refused with a message that names the kind of file; the check is on the real file, after links are followed, and a name that
  ends in a dot or a space is no document on macOS and Linux.
- **An Audio and video category in the Explorer filter.** *Docs and assets* includes it under the rules images have (next to a document, in a
  folder such as `assets`, `images`, `media`, `audio`, `video`, `sounds`, `movies` or `clips`, or linked from a document), and the hint says so. The tree
  has an icon each for images, sound and video.
- **Quick open and Search everywhere list pictures and media by name**, even when the filter hides them (a hidden folder still hides its files),
  **and folders**: the folders whose path has the words come after the files, with **Reveal in Explorer**. Picking one opens the folders above
  it, scrolls to it, opens it and puts the keyboard focus in the Explorer.
- **"Open with TrailMark" on folders in the Microsoft Store version.** Right-click a folder, or the background of a folder, and choose
  **Open with TrailMark**; on Windows 11 it is in the main menu, not under *Show more options*. The Store package now carries a small
  Explorer component for it (the one thing a package cannot do with the registry), worded in your Windows display language
  (English, Português, Español or Deutsch). Select several folders and each opens in its own window. **Settings → File manager** says
  that the entry comes from the Store package.

### Changed
- **Code block labels are easier to read.** The language label of a code block is muted instead of faint (it was below the contrast the design
  asks for), and the light theme's comment color is a little darker, for AA contrast on the code block's background. The reading view's code
  language also keeps the punctuation authors write after it (`rust,ignore`, `{python}`, `js:app.js`), which was dropped together with the language.
- `Ctrl+=`, `Ctrl+-` and `Ctrl+0` zoom the picture while a picture is on screen (and the zoom in the status bar is the picture's); for anything
  else they still zoom the text.
- **Open in external editor** on a sound or a video hands it to the system's application, as it already did for pictures and PDF files.
- **A new note is a note at once.** `+`, `Ctrl+Shift+N` and **New note** now always make a new note, even when the one on screen is still empty,
  like the `+` of any editor. The note is a real (empty) file as soon as it exists, so it is in the list as "Untitled note", is still there after
  a restart, and nothing removes it for you: you delete empty notes yourself, with Delete and Undo like any other note. Several empty notes can
  exist at the same time. Pin, Rename, Share, Move to project and Delete work on a note with no text.
- **Notes folder wording.** The message about a missing notes folder now says the folder *you chose* is not there, and why TrailMark does not
  make an empty one in its place.
- **Bring the right panel back from the corner it left.** While the right panel is collapsed, a **Show right panel** button (`«`) sits in the
  top-right corner of the document header, where the collapse button (`»`) was, so the pointer that just collapsed the panel is already over
  the button that brings it back, at the width it had. It carries the same dot as the activity bar's panel toggle when something new
  arrived while the panel was collapsed. `Ctrl+Alt+B` works as before; Simple mode has no panel and shows nothing.

### Fixed
- **Live changes: an edited block is no longer also shown as removed.** When a save changed a table (an edited cell and a new row) and the
  paragraph after it (a reworded sentence), both blocks glowed, but a "2 blocks removed" marker also appeared below them for their old
  versions, although nothing had been removed. An edited block is now matched with its old version and only glows. A block that moved glows
  where it landed, with no marker. The marker stays for blocks that were really deleted, so a deletion next to a change shows exactly one.
  A block rewritten beyond recognition is still shown as removed and added.
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
