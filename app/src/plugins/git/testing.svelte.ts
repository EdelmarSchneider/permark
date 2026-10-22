// Reactive stand-ins for what the members store reads from its window (tests).

import type { Looking } from "../../core/projects/store.svelte";

export class WindowParts {
  looking = $state.raw<Looking>({ glance: [], detail: [] });
  document = $state<string | null>(null);
  view = $state("7");
  shown = $state(true);
  root = $state<string | null>(null);

  constructor(root: string | null) {
    this.root = root;
  }
}
