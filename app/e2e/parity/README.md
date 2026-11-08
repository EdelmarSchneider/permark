# Parity probe: Edit looks like View

`npm run ux:parity` (in `app/`) opens the mock showcase documents (`README.md`, `docs/guide.md`, `docs/handbook.md`, `CLAUDE.md`) in a 1200 x 900 headless Chromium, in the light and the dark theme. For each document it reads the page in View, switches to Edit with the header's mode switch, and compares the two. Before reading a mode, it scrolls through the document once and back to the top, so math and diagrams, which are drawn when they come near the screen, are drawn in both modes.

What it compares:

- The content column (left edge, width).
- Blocks, paired by kind and text: height, the gap to the next block, and the frame (border, radius, background) of code blocks, tables and prompt boxes. A prompt box counts as three parts: its frame, its summary row, and the blocks it holds, each compared on its own.
- Elements inside each pair of blocks, paired by order within their type (headings, paragraphs, list items by kind, tables, rows, cells, code, links, file references, variables, quotes, rules, pictures, summaries, the code block's language label, HTML written in a line: `kbd`, `sub`, `sup`, `mark`, `br`): font, size, weight, line height, colors, margins, padding, border, radius, underline, and the height of list items and table rows.
- Entering Edit: nothing is selected, the page stays where View was, and the toolbar fits on one row.

Lengths may differ by 1 px (the scroll position by 2 px); colors must match exactly. Blocks the visual editor shows as source count once each; blocks it keeps as written but draws as View are compared like View's blocks.

Differences from features not yet implemented in Edit are listed in `known-gaps.json`, each tagged with its feature `area`. The probe exits with 1 when it finds a difference that is not on that list, and names the listed gaps it no longer sees, so the list only shrinks. The full report is written to `permark-ux-parity/report.json` in the system's temp folder.

- The front matter opened for editing: its text (dates in words) and its chips read as View's card.

Options: `--doc <path>` (repeatable), `--theme light|dark`, `--url <running dev:mock url>`, `--out <file>`, `--all` (list the known gaps too).

## In the real app

The mock app runs the real renderer's output, but not the real backend or the real window. To compare in the real app:

- `npm run ux:parity -- --app debug` starts the debug build (`permark.exe` in `CARGO_TARGET_DIR/debug`, or `src-tauri/target/debug`) on a copy of the fixture documents in the system's temp folder, laid out as the mock project. Build it first with `npx tauri build --debug --no-bundle`. The probe turns on the web view's remote debugging on a free local port (`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=<port>`), connects to it, compares, then closes the app and deletes the copy. `--app <exe>` starts another build; `--project <dir>` uses a folder of yours instead of the copy (it must hold the documents compared).
- `npm run ux:parity -- --cdp http://127.0.0.1:<port>` uses an app already started with that variable set, its project open on the documents compared. Nothing is started or closed.

A debug build keeps its settings and session in the `PerMark-dev` profile, never in your own. The probe switches the theme through the app's theme setting and puts it back when done. The page is the app's window as it is (its size is in the report); a PerMark debug build already running takes the folder itself, so close it first or use `--cdp`. The real app runs on Windows (WebView2).
