# Calibration fixtures (organizer-only)

Reviewers never open this folder before writing their findings: it names the bugs the calibration hides. Format, runner and rules: spec `ux-probe.md`. Steps stay unnamed (`step-<n>`), so a scenario handed to a reviewer says nothing about the bug.

## Cases

Runs: `npm run ux:calibrate` (default `--at all`: before + after + now). Status as of the commit that last touched this file.

| Fixture | Bug | Fix | before | after \| now |
|---|---|---|---|---|
| `chip-focus-ring` | ring stays on a chip no longer active after key switch | 563aaa9 | `focus-stale-in-group` | absent (`focus-lost`: chip lets go by design) |
| `header-over-panel` | doc header over outline \| live feed, narrow window | 71f2fb8 | `paints-over-region` | absent |
| `scrollbar-invisible` | thin \| invisible scroll bar | 71f2fb8 | `scrollbar-invisible` | thumb 8 px |
| `copy-not-in-tree` | copied file not in tree until reload | 92b6f85 | `no-change` | file listed at once |
| `review-last-commit` | "the last commit" without git does nothing | 72dec97 | `no-effect-beyond-control` | absent (choice explained inline) |
| `review-esc` | Done tooltip says Esc; Esc does nothing | 72dec97 | `dead-key-promised` | absent |

## Simulated, not literal (fixture `reproduction`)

- `chip-focus-ring`: mock = one page for all tabs; the real app has one webview per tab.
- `copy-not-in-tree`: the second tab's tree news is sent by seed `foreignTreeNews` (what the backend broadcasts for another tab's project); the copy itself is seed `importFiles` (backend copy, as a drop does).
- `review-last-commit`: seed `failPlugin` answers `review.open` basis `commit` like the backend without git (`unavailable` `noBasis`).
- Not reproducible in mock: none so far; a future case -> `not-reproducible.json` (`id`, `bug`, `reason`).

## Flag -> bug class it was made for

| Flag | Bug class |
|---|---|
| `focus-obscured` | header over focused item |
| `focus-partly-obscured` | same, partial |
| `focus-clipped` | hidden focus |
| `focus-no-indicator` | invisible focus |
| `focus-stale-in-group` | focus ring on wrong chip |
| `focus-lost` | keys go nowhere |
| `paints-over-region` | header over outline \| live feed |
| `region-overlap` | layout collision |
| `control-covered` | unclickable control |
| `dead-control` | control does nothing |
| `no-effect-beyond-control` | option toggles, nothing follows |
| `dead-key` | key does nothing |
| `dead-key-promised` | tooltip promises a key with no handler |
| `no-change` | list not updated |
| `scrollbar-invisible` | thin \| invisible scrollbar |
| `scrollbar-thin` | thin scrollbar |
| `axe:<rule>` | a11y floor |
