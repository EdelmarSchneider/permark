// Plugin `plantuml`: ```` ```plantuml ```` (and `puml`) blocks drawn in the page, with no Java and no server.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "plantuml", kind: "embed", dependsOn: [] },
  activate(cx) {
    const view = () => import("./Diagram.svelte");
    const offs = ["plantuml", "puml"].map((language) =>
      cx.contribute({
        kind: "embed",
        id: `plantuml.${language}`,
        language,
        requires: NO_REQUIREMENT,
        view,
      }),
    );
    return () => offs.forEach((off) => off());
  },
});
