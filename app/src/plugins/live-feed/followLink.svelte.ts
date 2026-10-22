// Follow is a plugin of its own that depends on this one: the feed reaches its store by a dynamic import, only while
// plugin `follow` is on (the Follow button of the Changes header and of the toast).

import { conditions } from "../../core/conditions/context.svelte";
import type { FollowStore } from "../follow/store.svelte";

let loaded = $state.raw<FollowStore | null>(null);
let asked = false;

/** The follow store while plugin `follow` is on (reactive; `null`: off, or not loaded yet). */
export function followStore(): FollowStore | null {
  if (!conditions.current.plugins.includes("follow")) return null;
  if (!asked) {
    asked = true;
    void import("../follow/store.svelte").then((module) => {
      loaded = module.follow;
    });
  }
  return loaded;
}
