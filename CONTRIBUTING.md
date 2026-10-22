# Contributing to PerMark

Thanks for your interest. PerMark is created and maintained by Edelmar Schneider, who sets its direction and has
the final say on what gets merged. Issues, ideas and pull requests are welcome.

## Before you start
- For anything bigger than a small fix, open an issue first so we can agree on the approach.
- The product is tool-agnostic: features rely on the file system and git, never on one specific AI tool.
- Every user-facing string goes through the language tables (see [Translations](#translations)). Code, docs and commit
  messages are in American English, written to be clear for readers who are not native speakers.
- Please call PerMark a "Markdown app" or a "unified document workspace", never an IDE.

## Build and run
The program lives in `app/`. You need:
- [Node.js](https://nodejs.org) 24 or newer.
- [Rust](https://rustup.rs). Do not pick a version: `app/rust-toolchain.toml` pins one, and `rustup` installs it the first
  time you build inside `app/`.
- The [Tauri prerequisites](https://v2.tauri.app/start/prerequisites/) for your system (on Linux: the WebKitGTK and GTK
  development packages).

```bash
cd app
npm ci
npm run tauri dev      # the app, with hot reload
npm run tauri build    # installers for your system
npm run dev:mock       # only the interface, in a browser, with a pretend backend (no Rust needed)
```

The code is in two halves that talk through typed commands and events: `app/src-tauri/` (Rust: a `core` crate, one crate
per plugin in `crates/plugins/`, and the app that joins them) and `app/src/` (Svelte 5 and TypeScript: `core/` and
`plugins/`). A feature that can be switched off lives in a plugin; what every window needs lives in the core.

## Workflow
- git-flow: branch from `develop` (`feature/<name>`), and open the pull request against `develop`.
- Conventional Commits: `feat(render): ...`, `fix(watch): ...`, `docs: ...`.
- The quality gate must pass, with no warnings. It is the same one CI runs on Windows, macOS and Linux:

```bash
cd app
npm run check   # version, rustfmt, prettier, build, clippy -D warnings, svelte-check, cargo test, vitest
npm run fmt     # fixes formatting (rustfmt and prettier)
```

- New behavior comes with tests, and so does a failure it must survive (bad input, mixed line endings, unusual paths).
- A change to what a file looks like after saving needs a test that proves every byte the person did not touch stays the
  same.
- Types shared with the interface are generated from Rust when `cargo test` runs. Commit the result when it changes.
- Adding a Rust crate or an npm package: every release lists the licenses of the third-party software inside it
  (`THIRD-PARTY-NOTICES.md`, made by `npm run notices`, which needs [cargo-about](https://github.com/EmbarkStudios/cargo-about)).
  CI makes it too, so a crate whose license is not in the `accepted` list of `app/src-tauri/about.toml`, or an npm package
  that names no license, fails the build. Check that the license allows redistribution in a program, then add it.
- The version number lives in `app/package.json`. `npm run version:sync` copies it to the other files; you rarely need it.

## Translations
PerMark speaks English, Português (Brasil), Español, Deutsch, 简体中文 and Italiano. English is the source language and
the fallback.

- **Where.** One file per language in `app/src/core/i18n/locales/`: `en.ts`, `pt-BR.ts`, `es.ts`, `de.ts`, `zh-CN.ts`,
  `it.ts`. Each is a flat list of `"area.key": "text"` pairs. The comment at the top of a file states the tone of that
  language (for example how buttons and hints address the reader); please follow it.
- **Fixing a translation.** Edit the text in the language's file and run `npm run check`. Nothing else changes.
- **Adding or renaming a string.** Add the key to `en.ts` and to all five other files in the same pull request, with a
  real translation (not the English text). Use `t("area.key")` in the code; `t("area.key", { name })` fills a
  `{name}` placeholder.
- **Parity test.** `npm run check` fails when a language lacks a key or has an extra one, when a placeholder differs from
  the English one, when a text is empty, or when `app.name` is not "PerMark". It also fails to compile when code uses a
  key that English does not have.
- **Plurals.** A text that depends on a number has two keys, `area.key.one` and `area.key.other`, and code calls
  `tn("area.key", count)`; `{n}` is the count. The plural rules of the active language pick the key: "one" where the
  language calls the count singular, "other" for every other count. Every language file has both keys (Chinese has a
  single plural form, so its `.one` text is never shown, but it must exist and make sense). Write the number as `{n}`.
- **Wording.** Keep product names and technical terms as they are (PerMark, Markdown, Git, MCP). Keep a text short enough
  for a button or a status bar, and say what happens in plain words.
- **A new language** touches more than the table: the language list in `locales.ts`, the Rust language setting, the
  Windows installer texts (`app/src-tauri/installer/hooks.nsh` and `tauri.conf.json`) and the Store manifest
  (`app/src-tauri/msstore/AppxManifest.xml`). Please open an issue first.

## Sign your commits (DCO)
By contributing you certify the [Developer Certificate of Origin](https://developercertificate.org/): that you wrote
the change, or otherwise have the right to submit it under the project's licenses. Add a sign-off line to each commit:

```bash
git commit -s -m "fix(links): decode %20 in anchors"
```

which appends `Signed-off-by: Your Name <you@example.com>`.

## Licensing of contributions
Unless you state otherwise, any contribution you submit is licensed under both [MIT](LICENSE-MIT) and
[Apache-2.0](LICENSE-APACHE), the same as the project, without additional terms. The PerMark name and logo are covered
by [TRADEMARKS.md](TRADEMARKS.md).
