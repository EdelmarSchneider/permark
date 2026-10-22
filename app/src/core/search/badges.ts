// The marks plugins put on a file (a git letter, an unread dot), for the file rows of search results: the same as
// the Explorer's (UX regressions #10). Reads reactive state: a row follows it.

import { conditions } from "../conditions/context.svelte";
import { contributions } from "../plugins/instance.svelte";
import type { TreeBadge } from "../plugins/registry.svelte";

export function badgesOf(path: string): TreeBadge[] {
  return contributions.visible("treeBadge", conditions.current).flatMap((b) => {
    const badge = b.badge(path, false);
    return badge === null ? [] : [badge];
  });
}
