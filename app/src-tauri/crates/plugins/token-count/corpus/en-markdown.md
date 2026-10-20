## Where to start

### Build with AI agents?

Your agents write the plans, specs and reports. Your job is to read them, judge them and steer. PerMark is the working environment for that
job: open the project folder and keep it next to your terminal. It doesn't run your agent; it shows you what your agent writes.

- **Watch.** Changed passages glow as your agent writes, removed ones leave a marker, and a live feed lists every file it
  touches, across git worktrees too. A reload keeps your place.
- **Review.** Unread dots mark every document that changed since you last read it, like new mail. Git badges and a "since your
  last visit" view still work after the agent has committed.
- **Find.** One search for every file, section, tag and command, and code references like `src/main.rs:42` that open at the
  line in the editor you choose.
- **Steer.** Select a passage, press `C` and write a comment. With the optional [agent connection](#use-with-an-ai-agent) (MCP),
  your agent reads your open comments, marks them done with a reply, and knows which document, section and words you mean when
  you say "rewrite this".
- **Edit next to it, safely.** Only the blocks you change are rewritten, and if your agent changes a file while you edit it, you
  choose **Reload theirs** or **Keep mine**.

More in [Follow your agents live](#-follow-your-agents-live) and [Use with an AI agent](#use-with-an-ai-agent).

### Got a .md file?

Your AI, a colleague or a download gives you a `.md` file. Open it with PerMark and read it as a page, not a wall of symbols.

- **It reads like a page.** Tables, checklists, highlighted code, Mermaid diagrams, math, alerts and footnotes, in light or dark,
  with three reading widths and a focus mode.
- **Links work.** Relative links and anchors open in the app, with back and forward and an outline of the headings.
- **You can fix it.** Edit visually, with a toolbar, or in Source. Only the blocks you changed are saved, and the rest of the file
  stays exactly as it was.
- **Notes that save themselves.** Press `Ctrl+Shift+N` in PerMark and type. No name to choose, no folder to pick, and closing
  never asks. Pin the notes you keep, bring back an earlier version, and copy a drafted prompt to your AI as clean Markdown.
- **Your files are safe.** A file you open is listed next to your notes and written only when you press `Ctrl+S`. PerMark is
  listed under **Open with** for Markdown files and never makes itself your default app: that choice stays yours.

More in [Reading that feels like a book](#-reading-that-feels-like-a-book) and [Notes](#-notes).

Either way, PerMark only relies on your **filesystem and git**. It works the same with Claude Code, opencode, Cursor, Aider,
Codex or Gemini CLI, or with none.

## Why Markdown

**Markdown was made for readers. So is PerMark.** It was designed in 2004 so that plain text reads well as it is. Twenty years
later, AI adopted it for the same reason: it is structured enough for a machine and clear enough for a person. PerMark is the
reader's tool for it.

It is also an opinion: many documents that live in Word or PDF today would be better as Markdown. They would be smaller,
comparable line by line in git, readable by any AI, and never locked inside one app.

## Features

### 📖 Reading that feels like a book
- GitHub-flavored Markdown: tables, task lists, footnotes, alerts (`> [!NOTE]`), description lists
- **Mermaid** diagrams, **KaTeX** math, syntax-highlighted code with copy buttons
- **Code blocks are highlighted for about 190 languages.** Name the language after the opening backticks, for example `` ```python ``, and
  the block is colored and labelled in its corner. The usual short names work (`js`, `ts`, `py`, `sh`, `ps1`, `c#`, `c++`, `yml`, `rs`...),
  and so do fences with attributes (`` ```rust title="main.rs" ``). A block with no language is colored only when its language is
  unmistakable, and is then labelled *auto*; otherwise it stays plain. Edit and Source color the code too while you edit
- Frontmatter shown as a clean metadata card
- Light and dark themes (with a softer paper, sage or gray tone for the light one), adjustable zoom, and a focus mode (`Ctrl+Alt+F`) that leaves only the page
- Three **reading widths**: Comfortable (a book-like column), Wide and Full. Wide tables, diagrams and images take the room
  they need while the text stays at a comfortable measure
- JSON, XML and plain text through an extensible renderer registry

### 🧭 Navigate like a wiki
- Open a **folder** as a project (tree, search, live feed) or a **single file** for a distraction-free reader
- Relative links, anchors and images resolve inside the app, with back and forward history. **Right-click (or press and hold) Back or
  Forward** to list the documents on that side and jump straight to one, like in a browser; **Go to a recent place...** in the command
  palette (`Ctrl+Shift+P`) lists your whole trail
- An **Outline** of the open document's headings (`Ctrl+Alt+O`): the section you're reading is marked, click a heading to jump,
  collapse sections you don't need. It keeps working while you edit, and a dot marks the sections an agent just changed
- A **filter** for the Explorer (Documents, Docs and assets, Everything, or your own patterns) keeps a code repository's tree to the
  documents that matter, and tells you how many files it hides
- Workspaces that collect several repositories show each one, even when the parent's `.gitignore` leaves it out. Hidden
  files and files ignored by git have their own switches in the filter, off by default
- The window opens where you left it, with its own size and place for each project, and its title names the document and the project,
  so several windows are easy to tell apart in the taskbar and `Alt+Tab`
- The Explorer **follows the file you open**, however you got there: quick open, a link, search, the feed, back and forward
- Quick open (`Ctrl+P`), find in page (`Ctrl+F`), session restore and recent projects
- **Code references are links.** `src/main.rs:42`, `lib.rs#L10-L20` or `` `docs/plan.md` `` in a document opens the file at the line:
  Markdown in PerMark, anything else in your editor (VS Code, Cursor, Zed, Sublime Text, JetBrains IDEs or a command of your own,
  in **Settings → External editor**). Only names of files the project really has become links
- One process per project: `permark ~/code/my-project`

### 🖼️ Pictures, sound and video
- **Images open in the pane.** PNG, JPEG, GIF (animated), WebP, AVIF, BMP, ICO and SVG. A picture is fitted to the window; click it, or use the
  button, to see it at 100%, and `Ctrl+=`, `Ctrl+-` and `Ctrl+0` zoom it the way they zoom text (`Ctrl+0` fits it again). A checkerboard
  shows through transparency, in light and dark. The header gives the size in pixels and on disk. A picture that changes on disk
  (an agent regenerating a screenshot) reloads by itself. An SVG is drawn as an image, never inline, so nothing in it can run.
- **Audio and video play in the pane**, in the window's own player: MP3, WAV, OGG, Opus, FLAC, M4A, AAC and WebM audio; MP4, M4V, WebM,
  OGV and MOV video. A long file is read in parts as it plays, so it starts at once, seeks freely and is never loaded whole. A format or
  codec the window cannot play (MKV, AVI, WMV and the like) says so, and offers the program your system has for it.
- **Documents can use them.** `<video>` and `<audio>` tags play in place, next to the document, and a link to a media file opens it in the
  pane. Nothing starts by itself: `autoplay` is dropped, and a player only reads the length of its file until you press play.
- **Open with default app** hands a file the pane cannot show to the program your system has for its type: a button in the pane, an item
  in the Explorer's right-click menu, and a command (`Ctrl+Shift+P`). Programs and scripts (`.exe`, `.bat`, `.ps1`, `.sh`, `.app`,
  `.lnk`, installers, Office documents that can hold macros such as `.docm`, `.doc` and `.xls`, and the like) are never opened from a click; PerMark says which kind of file it refused and offers
  to show it in the file manager. Files outside the open folder are never served, and a link that leaves it is not followed.
- **Open in browser** for `.html` and `.htm` files: in the Explorer's right-click menu, in the **...** menu of the open page, and in the
  command palette. The page opens in the application your system uses for web pages (your browser, on almost every computer); PerMark
  does not draw web pages itself.
- **The Explorer filter has an Audio and video category.** *Docs and assets* shows them by the same rules as images: next to a document, in
  a folder named like `assets`, `images`, `video`, `audio` or `clips`, or when a document links to them. Quick open and Search everywhere
  find pictures, sound and video by name whatever the filter hides, and list **folders** too: pick one to reveal it in the Explorer.
