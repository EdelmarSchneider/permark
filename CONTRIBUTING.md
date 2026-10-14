# Contributing to TrailMark

Thanks for your interest. TrailMark is created and maintained by Edelmar Schneider, who sets its direction and has
the final say on what gets merged. Issues, ideas and pull requests are welcome.

## Before you start
- For anything bigger than a small fix, open an issue first so we can agree on the approach.
- The product is tool-agnostic: features rely on the file system and git, never on one specific AI tool.
- Every user-facing string goes through `src/lib/i18n/`. Code, docs and commit messages are in English.

## Workflow
- git-flow: branch from `develop` (`feature/<name>`), and open the pull request against `develop`.
- Conventional Commits: `feat(render): ...`, `fix(watch): ...`, `docs: ...`.
- The quality gate must pass:

```bash
npm run check   # rustfmt, clippy -D warnings, cargo test, svelte-check, vitest
```

## Sign your commits (DCO)
By contributing you certify the [Developer Certificate of Origin](https://developercertificate.org/): that you wrote
the change, or otherwise have the right to submit it under the project's licenses. Add a sign-off line to each commit:

```bash
git commit -s -m "fix(links): decode %20 in anchors"
```

which appends `Signed-off-by: Your Name <you@example.com>`.

## Licensing of contributions
Unless you state otherwise, any contribution you submit is licensed under both [MIT](LICENSE-MIT) and
[Apache-2.0](LICENSE-APACHE), the same as the project, without additional terms. The TrailMark name and logo are covered
by [TRADEMARKS.md](TRADEMARKS.md).
