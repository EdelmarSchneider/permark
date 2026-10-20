---
tags: [plan]
---
# Plan

The plan for the first release of Atlas.

## Goals

- Sync notes between two devices in under five seconds.
- Keep every note readable as plain Markdown.
- Never lose an edit when two devices change the same note.

## Rollout

Ship the beta on Friday to the first ten teams. Collect feedback for two weeks, then open the waiting list.

### Steps

1. Freeze the protocol.
2. Run the retry policy tests.
3. Publish the beta build.

## Risks

- The retry policy may flood the server after a long outage.
- password: hunter2 is still in the old deploy notes.

## Open questions

Who owns the release notes? See [the spec](spec.md#conflicts).
