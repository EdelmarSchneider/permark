// Plugin `media`: pictures, sound and video shown in place.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "media", kind: "format", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "viewer",
      id: "media",
      claims: (name) =>
        /\.(png|jpe?g|gif|webp|avif|bmp|ico|svg|mp3|wav|og[ga]|opus|flac|m4a|aac|weba|wma|aiff?|mka|mp4|m4v|webm|ogv|mov|mkv|avi|wmv|flv|mpe?g|3gp)$/i.test(
          name,
        ),
      requires: NO_REQUIREMENT,
      view: () => import("./MediaView.svelte"),
    });
  },
});
