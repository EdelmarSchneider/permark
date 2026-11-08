# Parity probe: Edit looks like View

`npm run ux:parity` (in `app/`) opens the mock showcase documents (`README.md`, `docs/guide.md`, `docs/handbook.md`, `CLAUDE.md`) in a 1200 x 900 headless Chromium, in the light and the dark theme. For each document it reads the page in View, switches to Edit with the header's mode switch, and compares the two.

What it compares:

- The content column (left edge, width).
- Blocks, paired by kind and text: height, the gap to the next block, and the frame (border, radius, background) of code blocks and tables.
- Elements inside each pair of blocks, paired by order within their type (headings, paragraphs, list items by kind, tables, rows, cells, code, links, quotes, rules, pictures, the code block's language label): font, size, weight, line height, colors, margins, padding, border, radius, underline, and the height of list items and table rows.
- Entering Edit: nothing is selected, the page stays where View was, and the toolbar fits on one row.

Lengths may differ by 1 px (the scroll position by 2 px); colors must match exactly. Blocks the visual editor shows as source count once each.

Differences that later work will close are listed in `known-gaps.json`, each tagged with its `phase`. The probe exits with 1 when it finds a difference that is not on that list, and names the listed gaps it no longer sees, so the list only shrinks. The full report is written to `permark-ux-parity/report.json` in the system's temp folder.

Options: `--doc <path>` (repeatable), `--theme light|dark`, `--url <running dev:mock url>`, `--out <file>`, `--all` (list the known gaps too).
