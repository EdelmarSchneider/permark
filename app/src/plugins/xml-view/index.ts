// Plugin `xml-view`: XML files as a read-only tree, the text one click away.

import { NO_REQUIREMENT } from "../../core/conditions/available";
import { definePlugin } from "../../core/plugins/registry.svelte";

export default definePlugin({
  manifest: { id: "xml-view", kind: "format", dependsOn: [] },
  activate(cx) {
    return cx.contribute({
      kind: "viewer",
      id: "xml",
      claims: (name) => /\.(xml|xsd|xsl|xslt|svg|plist|csproj|rss|atom)$/i.test(name),
      requires: NO_REQUIREMENT,
      view: () => import("./XmlView.svelte"),
    });
  },
});
