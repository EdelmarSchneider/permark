# Design: a color and an icon for a tag

A tag can have a small look of its own, so a reader sees at a glance what kind of document it is: a blue book for specs, an
orange flag for anything that needs review, a green check for what is done. A tag with neither looks as every tag always did.

**Principles**
- **Opt-in.** Chips stay neutral until someone chooses. The accent keeps its meaning (live changes), so the colors are their own
  palette and nothing is colored by default.
- **A fixed, small set.** Eight colors and twelve icons. Nothing free-form: no hex values, no emoji, no uploaded pictures. The
  look is always legible and always in tune with the theme.
- **Per project, shared with it.** The look belongs to the tag in this project, not to a file, and travels with the rest of the
  project's tags.

## 1. The palette and the icons

Colors: **gray, red, orange, yellow, green, teal, blue, purple.**
Icons (the app's own icon set): **star, bookmark, flag, lightbulb, check, alert, book-open, code, folder, user, calendar, link.**

Both lists live in `src-tauri/src/meta/style.rs` and `src/lib/tagStyle.ts`; a test compares them.

Each color is a design token, `--tag-<name>`, with a value for the light theme and one for the dark theme
(`src/styles/tokens.css`). A chip draws the tag's name and icon in that color on a tint of it (`--tag-tint`, 12% of the color over
the page background), with a border a little stronger than the tint. The tint is opaque, so the contrast does not depend on what
is behind the chip. The values keep at least 4.5:1 for the text on its tint, and for the mark in a list on every surface it can
sit on, in the dark theme and in the light theme with each tone (white, paper, sage, gray); a test holds them to it.

## 2. Where a tag shows its look

| Place | What shows |
| --- | --- |
| Chips: the strip above a document, the tag editor, the search boxes | Name and icon in the color, on the tint. A tag with an icon shows it in place of the "from the file" page icon. |
| Tags view (the list of tags) | A small mark before the name: the icon in the color, or a dot of the color when there is no icon. Once any listed tag has a look, every row keeps a slot for it so the names line up. |
| Tag suggestions (while typing a tag) | The same mark. |

The Explorer does not show a mark per file: a colored dot on many rows is noise next to the Git letters and the unread dot, and
the strip above the document already says it.

## 3. Choosing a look

In the Tags view, right-click a tag (or focus it and press the menu key or Shift+F10) and choose **Color and icon...**. A small
popover opens beside the row. It holds, from top to bottom:
- a **preview**: the tag's chip, as it looks right now;
- **Color**: a row of round swatches, "No color" first and then the eight colors, the chosen one ringed and ticked;
- **Icon**: a grid, "No icon" first and then the twelve icons, drawn in the chosen color so the two choices can be judged together.

- Every pick is saved at once and shows on the preview, in the list and on every chip. Escape, or a click outside, closes it.
- Each group is a radio group: one tab stop, the arrow keys (and Home and End) move and choose, Space and Enter choose. Every choice
  has a name for screen readers and a tooltip. The popover opens with the current color focused.
- While `.permark/meta.toml` cannot be read (a merge conflict), the choices are disabled and a note says why, like the rest of
  the app's tags.
- Any tag can be styled, including one that only exists in files' front matter.

## 4. Where it is kept

In the same file as the project's other tag data (`.permark/meta.toml` once the project has a `.permark/` folder, a file in
the app's data folder before that), as one small table per styled tag:

```toml
[tag_styles.spec]
color = "blue"
icon = "book-open"

[tag_styles."area/api"]
color = "teal"
```

- Keyed by the tag's name, the same normalized form the tags use (lowercase, words joined by `-`). Either key can be left out.
- A file without the table reads as "no looks". A color or icon this version does not know (written by a newer version) is not
  shown, but stays in the file when the file is written again; so does any other key in a tag's table.
- A style for a tag no file has any more is harmless: it is simply never asked about, and it comes back to life if the tag does.
- The backend answers two commands: `tag_styles` (every look) and `set_tag_style` (one tag's color and icon, `null` for none; a
  name outside the palette or the set means none). `meta-changed` tells every window when the file changes, here or on disk.
