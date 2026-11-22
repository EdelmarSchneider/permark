// @vitest-environment jsdom
// Against the real Mermaid (no stand-in): the keys the page sets for safety stay as set whatever a diagram's init
// directive asks. HTML labels would put HTML in a foreignObject, a DOMPurify config would widen what Mermaid lets
// through, absolute arrow markers carry the page's address into the drawing. A Mermaid version that reads its secure
// list differently fails here.
import { beforeAll, describe, expect, it } from "vitest";

const { drawMermaid } = await import("./engine");
const mermaid = (await import("mermaid")).default;

// the page's colors, which the palette is read from
beforeAll(() => {
  const style = document.createElement("style");
  style.textContent = `:root {
    --bg: #ffffff; --text: #1b1d23; --muted: #5d636e; --surface: #f7f7f8; --surface-2: #efeff1;
    --border: #e3e4e8; --border-strong: #c9ccd3; --measure: 760px;
    --tag-gray: #515962; --tag-red: #a81f26; --tag-orange: #9a4108; --tag-yellow: #7d5700;
    --tag-green: #146229; --tag-teal: #0a6560; --tag-blue: #1a58b8; --tag-purple: #6b3fc4;
  }`;
  document.head.append(style);
});

const DIRECTIVE = JSON.stringify({
  htmlLabels: true,
  flowchart: { htmlLabels: true },
  arrowMarkerAbsolute: true,
  dompurifyConfig: { ADD_TAGS: ["foreignObject", "iframe"], ADD_ATTR: ["onload"] },
  secure: [],
});

describe("a diagram's init directive", () => {
  it("cannot turn on HTML labels, a DOMPurify config of its own or absolute arrow markers", async () => {
    // the page's configuration in place (the drawing itself needs a layout jsdom lacks)
    await drawMermaid("flowchart LR\n  A --> B", "light", true);
    await mermaid.parse(`%%{init: ${DIRECTIVE}}%%\nflowchart LR\n  A --> B`);
    const config = mermaid.mermaidAPI.getConfig();
    expect(config.htmlLabels).toBe(false);
    expect(config.flowchart?.htmlLabels).toBe(false);
    expect(config.arrowMarkerAbsolute).toBe(false);
    expect(config.dompurifyConfig ?? {}).not.toHaveProperty("ADD_TAGS");
    expect(config.dompurifyConfig ?? {}).not.toHaveProperty("ADD_ATTR");
    expect(config.secure).toEqual(
      expect.arrayContaining(["dompurifyConfig", "htmlLabels", "arrowMarkerAbsolute"]),
    );
  });

  it("the same directive does reach a key the page left open (the check is not empty)", async () => {
    await drawMermaid("flowchart LR\n  A --> B", "light", true);
    await mermaid.parse(`%%{init: {"flowchart": {"curve": "step"}}}%%\nflowchart LR\n  A --> B`);
    expect(mermaid.mermaidAPI.getConfig().flowchart?.curve).toBe("step");
  });
});
