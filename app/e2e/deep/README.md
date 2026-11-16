# Deep suite

A slower set of checks than `npm run check`, run by hand every few days (or before a release) to see whether a change
broke how documents render or let something unsafe through. It is not part of the gate CI runs on every push.

```bash
cd app
npm run deep                       # everything, about 2 to 3 minutes on a recent machine
npm run deep -- --only render      # render goldens only
npm run deep -- --only security    # sanitizer properties and the attack page only
npm run deep -- --quick            # light theme, View only, fewer sanitizer cases
npm run deep -- --grep math        # only the browser tests whose title matches
```

It needs the app's dev dependencies (`npm ci`), Rust (the corpus is rendered with `cargo test`) and Playwright's
Chromium (`npx playwright install chromium`).

## What it checks

1. **Render the corpus.** The real Rust renderer renders every file of the corpus with every plugin on, the same way
   `cargo test` renders the mock documents (`render_corpus` in `src-tauri/src/plugins.rs`, ignored in a normal
   `cargo test`). The corpus is the repository's `demo/` folder plus the small targeted pages in `corpus/` here: every
   GitHub Markdown construct and PerMark extension, diagrams with syntax errors, front matter in YAML and TOML, CRLF and
   BOM files, right-to-left and CJK text, invisible characters, LLM-style output and an attack page.
2. **Sanitizer properties.** Generated hostile markup goes through the sanitizer (`sanitize_props.rs` in the core
   crate): nothing that runs or loads a page may survive, link schemes stay on the allowed list, and a second pass keeps
   the text and attributes. A normal `cargo test` runs a few cases; this suite runs thousands (`PROPTEST_CASES`).
3. **Deep server.** `vite --mode deep` serves the app on the mock backend with the rendered corpus in place of the mock
   documents, the corpus's own pictures and sounds, and the app's Content Security Policy on every response.
4. **Browser specs** (`*.spec.ts`, Playwright; vitest never picks them up):
   - `render.spec.ts`: every corpus page, drawn completely (the page is scrolled through once and every diagram and
     formula is finished), checked against
     - **text goldens** in `goldens/` (in the repository): the renderer's blocks, and a digest of the reading view
       after drawing (diagrams reduced to their kind and labels, formulas to their TeX, generated ids left out);
     - **pixel baselines** outside the repository: the first screen and each diagram, formula, table, callout, code
       block and front matter card, in View, light and dark, 1200 px wide; tables and code blocks again at 700 px; the
       first screen of Source and of Edit.
   - `security.spec.ts`: the attack page in View, Edit and Source; every link is hovered and clicked.

   Every page is guarded the whole time: a request that leaves the machine, a Content Security Policy violation, a
   dialog, an uncaught error or the attack canary (`window.__pmCanary`) fails the test. Animations are off, the caret is
   hidden, the clock is fixed and the pixel ratio is 1.

## Results

Each run writes to `permark-deep/<date-time>/` in the system's temp folder: `summary.md` (verdict, timings, failures),
`report/index.html` (Playwright's report with every difference) and `results/` (actual and difference images). The last
three runs are kept. The command exits with 1 when anything failed.

## Updating goldens and baselines

When a change is meant to alter how pages look:

```bash
npm run deep -- --update               # rewrites text goldens; differing or new shots become candidates
npm run deep -- --accept all           # or a pattern: --accept "demo__README/*" | --accept "**dark-view-diagram-*"
```

Text goldens are rewritten in place: review them with `git diff` like any other change. Pixel candidates go to
`<baselines>/pending/`, with `index.html` showing each one beside its baseline and the difference; `--accept` moves the
chosen ones over the baselines.

Pixel baselines are kept per operating system and browser, outside the repository: by default in
`deep-baselines/<os>-chromium/` next to the repository folder (`PERMARK_DEEP_BASELINES` moves it). `meta.json` there
records the Playwright version, browser build and OS build that drew them. When any of these differs, the run skips the
pixel comparison and says so instead of failing every shot: record them again with `--update`, then `--accept all`.

## Known issues

`known-issues.json` lists findings that are already known and not fixed yet (for example, a picture from the web that
Edit still tries to load, which only the Content Security Policy stops). Tests report them in the summary without
failing. Remove an entry when its fix lands.

## Environment

| Variable | Default | What |
|---|---|---|
| `PERMARK_DEEP_HOME` | `<temp>/permark-deep` | runs, rendered corpus, the server's cache |
| `PERMARK_DEEP_OUT` | `<home>/corpus` | the rendered corpus |
| `PERMARK_DEEP_CORPUS` | `demo` + `app/e2e/deep/corpus` | folders to render (a path list) |
| `PERMARK_DEEP_BASELINES` | `../deep-baselines/<os>-chromium` | pixel baselines |
| `PERMARK_DEEP_WORKERS` | 4 | browsers run in parallel |

## Adding a corpus page

Add a small `.md` file to `corpus/` (one topic per file; its pictures in `corpus/assets/`). The files are kept byte for
byte by git (`.gitattributes`), so line endings and BOMs stay as written. Then run `--update` and `--accept` for its new
shots, and commit the page with its text goldens.
