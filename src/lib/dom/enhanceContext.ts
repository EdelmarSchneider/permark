// The context the backend's HTML is enhanced with (code blocks, diagrams, math, images relative to the document), the same for the
// reading view and for the blocks Visual mode shows as they are read.

import { t } from "../i18n";
import { assetUrl } from "../platform";
import type { EnhanceContext } from "./enhance";

export function enhanceContextFor(path: string, theme: "light" | "dark"): EnhanceContext {
  return {
    docPath: path,
    theme,
    labels: {
      copyCode: t("doc.copyCode"),
      codeAuto: (name) => t("doc.codeAuto", { name }),
      codeAutoHint: t("doc.codeAutoHint"),
      anchorLabel: (name) => t("doc.anchorLabel", { name }),
      footnoteBackref: (ref) => t("doc.footnoteBackref", { ref }),
      diagramPending: t("doc.diagramRendering"),
      diagramFailed: t("doc.diagramFailed"),
      alertTitles: {
        note: t("alert.note"),
        tip: t("alert.tip"),
        important: t("alert.important"),
        warning: t("alert.warning"),
        caution: t("alert.caution"),
      },
    },
    resolveImage: async (imagePath) => assetUrl(imagePath),
  };
}
