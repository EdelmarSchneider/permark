// @vitest-environment jsdom
import { beforeEach, describe, expect, it } from "vitest";
import { diagrams, sanitizeSvg, viewBoxWidth } from "./diagram";

const SVG_NS = "http://www.w3.org/2000/svg";
const XLINK = "http://www.w3.org/1999/xlink";

const svg = (inner: string): string =>
  `<svg xmlns="${SVG_NS}" xmlns:xlink="${XLINK}" viewBox="0 0 100 50">${inner}</svg>`;

/**
 * The cleaned drawing as the page builds it: `{@html}` parses the markup as HTML, not as XML. Checked by attributes,
 * never by an `a[href]` selector: jsdom matches that selector on a link that has only `xlink:href`, Chromium does not.
 * `ownRoot`: the root id the engine was asked to draw with.
 */
function onPage(dirty: string, ownRoot?: string): Element {
  const clean = sanitizeSvg(dirty, ownRoot);
  if (clean === null) throw new Error("refused");
  const template = document.createElement("template");
  template.innerHTML = clean;
  const [root, ...more] = [...template.content.children];
  expect(more.map((el) => el.localName)).toEqual([]);
  if (root === undefined) throw new Error(`nothing drawn: ${clean}`);
  expect(root.namespaceURI).toBe(SVG_NS);
  return root;
}

/** Every link of the page with the attributes that could send it somewhere, as `name=value` (prefix kept). */
function links(root: Element): string[][] {
  return [...root.querySelectorAll("*")]
    .filter((el) => el.localName === "a")
    .map((link) =>
      [...link.attributes]
        .filter((attr) => /^(?:href|target|show)$/i.test(attr.localName))
        .map((attr) => `${attr.name}=${attr.value}`),
    );
}

/** The tag names the page holds inside the drawing. */
const tags = (root: Element): string[] => [...root.querySelectorAll("*")].map((el) => el.localName);

describe("sanitizeSvg links", () => {
  it("a link written with xlink:href only (Mermaid) leaves as one plain href, without its target", () => {
    const page = onPage(svg('<a xlink:href="docs/x.md" target="_blank"><text>x</text></a>'));
    expect(links(page)).toEqual([["href=docs/x.md"]]);
    expect(page.querySelector("text")?.textContent).toBe("x");
  });

  it("a link written with href, xlink:href and target (PlantUML) leaves with one plain href", () => {
    // the engine's own shape, with the source it keeps in a processing instruction
    const page = onPage(
      svg(
        `<g class="entity"><a target="_top" href="x.md" xmlns:ns1="${XLINK}" ns1:href="x.md" ns1:type="simple" ns1:actuate="onRequest" ns1:show="new" title="x.md" ns1:title="x.md"><rect width="5" height="5"/><text>Foo</text></a></g><?plantuml-src AyaioKbLyArKA04nk99p4ekB5TpoynMYeolq?>`,
      ),
    );
    expect(links(page)).toEqual([["href=x.md"]]);
    expect(page.querySelector("rect")?.parentElement?.getAttribute("title")).toBe("x.md");
  });

  it("names in any case: an <A> element and an XLINK:HREF | HREF attribute", () => {
    expect(links(onPage(svg('<A xlink:href="x.md"><text>x</text></A>')))).toEqual([["href=x.md"]]);
    const upper = `<svg xmlns="${SVG_NS}" xmlns:XLINK="${XLINK}"><a XLINK:HREF="x.md"><text>x</text></a><a HREF="y.md" TARGET="_blank"><text>y</text></a></svg>`;
    expect(links(onPage(upper))).toEqual([["href=x.md"], ["href=y.md"]]);
  });

  it("a drawing that writes xlink:href without declaring the prefix (Mermaid) still draws", () => {
    const mermaid = `<svg id="mermaid-1" width="100%" xmlns="${SVG_NS}" class="flowchart" style="max-width: 80px;" viewBox="0 0 80 40" role="graphics-document document" aria-roledescription="flowchart-v2"><g><a xlink:href="other.md"><text>x</text></a></g></svg>`;
    expect(links(onPage(mermaid))).toEqual([["href=other.md"]]);
    // after a prolog that names an svg, with a `>` in an attribute value and line breaks between attributes
    const prolog = `<?xml version="1.0"?>\n<!-- an <svg> note -->\n<svg\n  aria-label="a > b"\n  xmlns="${SVG_NS}"><a xlink:href="x.md"><text>x</text></a></svg>`;
    expect(links(onPage(prolog))).toEqual([["href=x.md"]]);
    // declared in single quotes: not declared twice
    const quoted = `<svg xmlns="${SVG_NS}" xmlns:xlink='${XLINK}'><a xlink:href="y.md"><text>y</text></a></svg>`;
    expect(links(onPage(quoted))).toEqual([["href=y.md"]]);
  });

  it("the xlink declaration goes on the root only, and only when the prefix is used", () => {
    const nested = onPage(
      `<svg xmlns="${SVG_NS}"><svg><a xlink:href="x.md"><text>x</text></a></svg></svg>`,
    );
    const inner = nested.querySelector("svg");
    expect(inner?.getAttributeNames()).toEqual([]);
    expect(links(nested)).toEqual([["href=x.md"]]);
    const plain = `<svg xmlns="${SVG_NS}" viewBox="0 0 1 1"><rect width="1" height="1"/></svg>`;
    expect(sanitizeSvg(plain)).toBe(plain);
  });

  it("a link whose href and xlink:href differ goes nowhere; its content stays", () => {
    const page = onPage(svg('<a href="a.md" xlink:href="b.md"><text>kept</text></a>'));
    expect(links(page)).toEqual([[]]);
    expect(page.querySelector("text")?.textContent).toBe("kept");
  });

  it("keeps web, mail, in-page and project links, trimmed", () => {
    const kept: [string, string][] = [
      [" https://example.com/a ", "https://example.com/a"],
      ["HTTP://example.com", "HTTP://example.com"],
      ["MAILTO:a@b.c", "MAILTO:a@b.c"],
      [" #sec", "#sec"],
      ["../a.md ", "../a.md"],
      ["&#xA0;notes/my doc.md", "notes/my doc.md"],
      ["notes/my doc.md", "notes/my doc.md"],
    ];
    for (const [written, href] of kept) {
      expect(links(onPage(svg(`<a xlink:href="${written}"><text>k</text></a>`))), written).toEqual([
        [`href=${href}`],
      ]);
    }
  });

  it("any other address leaves the link going nowhere, its content kept", () => {
    const refused = [
      "javascript:alert(1)",
      "JaVaScRiPt:alert(1)",
      "java&#9;script:alert(1)",
      "&#9;javascript:alert(1)",
      "&#xA;javascript:alert(1)",
      "&#xA0;javascript:alert(1)",
      // a browser drops tabs and line breaks inside an address: `ht\ttps:` is `https:` there
      "ht&#9;tps://x",
      "mail&#xA;to:a@b.c",
      "doc&#xD;.md",
      "x&#x7F;.md",
      "vbscript:msgbox",
      "data:text/html,x",
      "file:///C:/x.md",
      "FILE:///C:/x.md",
      "tauri://localhost/x",
      "about:blank",
      "C:/x.md",
      String.raw`c:\x.md`,
      String.raw`\\server\share\x.md`,
      "//host/x.md",
      String.raw`/\host/x.md`,
      String.raw`\/host/x.md`,
      "tel:123",
      "ftp://host/x",
      "cid:x",
      "",
      "   ",
    ];
    for (const value of refused) {
      for (const attr of ["href", "xlink:href"]) {
        const page = onPage(svg(`<a ${attr}="${value}"><text>child</text></a>`));
        expect(links(page), `${attr}="${value}"`).toEqual([[]]);
        expect(page.querySelector("text")?.textContent).toBe("child");
      }
    }
  });
});

describe("sanitizeSvg", () => {
  it("keeps a plain drawing", () => {
    const clean = sanitizeSvg(
      svg('<rect width="10" height="10" fill="red"/><text x="1" y="2">hi</text>'),
    );
    expect(clean).toContain("<rect");
    expect(clean).toContain("hi");
  });

  it("drops scripts, foreign content and event handlers", () => {
    const clean = sanitizeSvg(
      svg(
        '<script>alert(1)</script><foreignObject><div>x</div></foreignObject><rect onclick="x()" onload="y()" width="1" height="1"/><iframe src="x"/>',
      ),
    );
    expect(clean).not.toBeNull();
    for (const bad of ["script", "foreignObject", "onclick", "onload", "iframe", "alert"]) {
      expect(clean).not.toContain(bad);
    }
    expect(clean).toContain("<rect");
  });

  it("removes remote and script references but keeps in-page ones", () => {
    const clean = sanitizeSvg(
      svg(
        '<rect id="shape" width="1" height="1"/><image href="https://example.com/x.png"/><image xlink:href="data:image/png;base64,AAAA" xmlns:xlink="http://www.w3.org/1999/xlink"/><a href="javascript:alert(1)"><text>l</text></a><use href="#shape"/><image href="//evil/x"/>',
      ),
    );
    expect(clean).not.toContain("example.com");
    expect(clean).not.toContain("javascript");
    expect(clean).not.toContain("evil");
    expect(clean).not.toContain("data:image");
    expect(clean).toContain('href="#shape"');
    expect(clean).toContain("<text>l</text>");
  });

  it("drops animations that could set a link and script addresses written with spaces", () => {
    const clean = sanitizeSvg(
      svg(
        '<a><set attributeName="href" to="javascript:alert(1)"/><animate attributeName="href" values="javascript:x"/><text>s</text></a><a href="java\tscript:alert(2)"><text>t</text></a><a href=" javascript:alert(3)"><text>u</text></a>',
      ),
    );
    expect(clean).not.toContain("<set");
    expect(clean).not.toContain("<animate");
    expect(clean).not.toContain("script:");
    expect(clean).toContain("<text>t</text>");
  });

  it("references in any mix of slashes, file addresses, or a bad one beside a good one remove the element", () => {
    const page = onPage(
      svg(
        String.raw`<rect id="shape" width="1" height="1"/><use href="#shape"/><image href="\\evil\x.png"/><image href="/\evil/x.png"/><image href="file:///C:/s.png"/><use href="file:///x"/><image href="#shape" xlink:href="https://evil/x.png"/><image HREF="//evil/x.png"/><image href="pictures/ok.png"/>`,
      ),
    );
    expect(tags(page)).toEqual(["rect", "use"]);
    expect(page.querySelector("use")?.getAttribute("href")).toBe("#shape");
  });

  it("an element other than a link keeps only references into the drawing", () => {
    const page = onPage(
      svg(
        '<path id="p" d="M0 0"/><use href="#p"/><text><textPath href="#p">t</textPath></text><image href="pictures/ok.png"/><filter id="f"><feImage href="ok.png"/></filter><pattern id="q" xlink:href="other.svg#q"/><use href=" #p"/>',
      ),
    );
    expect(tags(page)).toEqual(["path", "use", "text", "textPath", "filter"]);
  });

  it("removes a <use> that would copy a link, a copy, or a part missing from the drawing", () => {
    // a click on a copied link reaches the page as a click on the <use>: no link guard sees a link there
    const page = onPage(
      svg(
        '<a id="l" href="x.md"><text>l</text></a><g id="holds"><A href="y.md"><text>y</text></A></g>' +
          '<rect id="shape" width="1" height="1"/><g id="chain"><use href="#shape"/></g><g id="%6C"/>' +
          '<use href="#l"/><use xlink:href="#holds"/><use href="#chain"/><use href="#missing"/>' +
          '<use href="#%6C"/><use href="#sh&#9;ape"/><use href="#shape" xlink:href="#l"/><use href="#shape"/>',
      ),
    );
    const uses = [...page.querySelectorAll("*")].filter((el) => el.localName === "use");
    expect(uses.map((use) => use.getAttribute("href"))).toEqual(["#shape", "#shape"]);
    expect(uses.every((use) => !use.hasAttributeNS(XLINK, "href"))).toBe(true);
  });

  it("removes processing instructions and comments, whose markup would come alive on the page", () => {
    const dirty = svg(
      '<?x ><meta http-equiv="refresh" content="0;url=/">?><!--><img src="x">--><g><text>t</text></g><?y ><img src="y">?>',
    );
    const clean = sanitizeSvg(dirty) ?? "";
    expect(clean).not.toContain("<?");
    expect(clean).not.toContain("<!--");
    expect(tags(onPage(dirty))).toEqual(["g", "text"]);
  });

  it("drops dangerous elements whatever the case of their names, and the root's handlers", () => {
    const page = onPage(
      `<svg xmlns="${SVG_NS}" onload="alert(1)" ONCLICK="x()"><SCRIPT>alert(1)</SCRIPT><FOREIGNOBJECT><div>x</div></FOREIGNOBJECT><SET attributeName="href" to="x.md"/><ANIMATE attributeName="href" values="x.md"/><animateMotion/><AnimateTransform/><Discard/><Meta http-equiv="refresh" content="0;url=/"/><LINK rel="stylesheet" href="x.css"/><base href="/"/><IFRAME/><Object/><embed/><style SRC="x.css"/><rect width="1" height="1"/></svg>`,
    );
    expect(tags(page)).toEqual(["rect"]);
    expect([...page.attributes].map((attr) => attr.name)).toEqual(["xmlns"]);
  });

  it("refuses markup the page would parse out of the drawing", () => {
    for (const inner of [
      "<p/>",
      '<img src="x"/>',
      "<table/><style>body{display:none}</style>",
      '<g><font color="red">x</font></g>',
      // the content of an SVG title is parsed as HTML
      '<title><img src="x"/></title>',
    ]) {
      expect(sanitizeSvg(svg(inner)), inner).toBeNull();
    }
  });

  it("a style whose CDATA holds a closing tag stays text on the page", () => {
    const page = onPage(
      `<svg id="d" xmlns="${SVG_NS}"><style><![CDATA[#d .a{content:"</style><img src=x onerror=alert(1)>"}]]></style><rect width="1" height="1"/></svg>`,
      "d",
    );
    expect(tags(page)).toEqual(["style", "rect"]);
    expect(page.querySelector("style")?.textContent).toContain("<img");
  });

  it("refuses a style in a description, which the page reads as HTML", () => {
    expect(sanitizeSvg(svg("<desc><style>x</style></desc>"))).toBeNull();
  });

  it("refuses a root that is not a plain SVG svg element", () => {
    const xhtml = '<svg xmlns="http://www.w3.org/1999/xhtml"><rect width="1" height="1"/></svg>';
    expect(sanitizeSvg(xhtml)).toBeNull();
    const prefixed = `<x:svg xmlns:x="${SVG_NS}"><x:rect width="1" height="1"/></x:svg>`;
    expect(sanitizeSvg(prefixed)).toBeNull();
  });

  it("a handler written with a prefix never becomes a handler on the page", () => {
    // the page's HTML parse keeps a prefixed name whole (`onx:click`): only a bare `on…` name is a handler there
    const page = onPage(
      svg(
        '<rect xmlns:onx="urn:x" onx:click="x()" ONY:load="y()" xmlns:ONY="urn:y" xmlns:z="urn:z" z:onclick="z()"/>',
      ),
    );
    const rect = page.querySelector("rect");
    expect(rect?.getAttributeNames().filter((name) => /^on[a-z]+$/i.test(name))).toEqual([]);
    expect(rect?.getAttributeNames().some((name) => /onclick/i.test(name))).toBe(false);
  });

  it("refuses what is not SVG or not well-formed", () => {
    expect(sanitizeSvg("<html><body>x</body></html>")).toBeNull();
    expect(sanitizeSvg("not xml")).toBeNull();
    expect(sanitizeSvg("<svg><g></svg>")).toBeNull();
    expect(sanitizeSvg("")).toBeNull();
  });

  it("accepts the doubled xmlns some engines write", () => {
    const doubled =
      '<svg xmlns="http://www.w3.org/2000/svg" xmlns="http://www.w3.org/2000/svg"><g/></svg>';
    expect(sanitizeSvg(doubled)).not.toBeNull();
  });
});

describe("sanitizeSvg CSS", () => {
  const ID = "pm-mermaid-1-abcd1234";
  /** The drawing's root in a selector: Mermaid starts every selector of its CSS with it. */
  const R = `#${ID}`;
  /** A drawing the way Mermaid writes one: its id on the root, its CSS in one `<style>` scoped to that id. */
  const drawing = (inner: string, rootStyle = "max-width: 80px;"): string =>
    `<svg id="${ID}" width="100%" xmlns="${SVG_NS}" style="${rootStyle}" viewBox="0 0 80 40">${inner}</svg>`;
  const scoped = (css: string, rootStyle?: string): string =>
    drawing(`<style>${css}</style><g><rect class="node" width="10" height="10"/></g>`, rootStyle);
  const sheet = (page: Element): string | null | undefined =>
    page.querySelector("style")?.textContent;
  const styled = (css: string): Element => onPage(scoped(css), ID);

  it("a style reaches only the drawing: a rule for anything outside its root goes", () => {
    const cases: [string, string][] = [
      ["body{display:none}", ""],
      ["*{visibility:hidden}", ""],
      ['body::before{content:"x"}', ""],
      [":root{--bg:red}", ""],
      [`:root ${R}{fill:red}`, ""],
      [`#other ${R}{fill:red}`, ""],
      // a longer id is another element
      [`${R}x .node{fill:red}`, ""],
      // the root's siblings are the page's
      [`${R} ~ p{display:none}`, ""],
      [`${R}+p{display:none}`, ""],
      [`${R}:hover ~ p{display:none}`, ""],
      [`${R}.a:is(.b) ~ p{display:none}`, ""],
      // a drawing without its own rules keeps nothing
      ["{fill:red}", ""],
      [`${R} .node{fill:red}`, `${R} .node{fill:red}`],
      [`${R}{fill:red}`, `${R}{fill:red}`],
      [`${R}>g,${R}.a:nth-child(2n+1) .b{fill:red}`, `${R}>g,${R}.a:nth-child(2n+1) .b{fill:red}`],
      // only the selectors outside go from a list
      [`${R} .a, body{fill:red}`, `${R} .a{fill:red}`],
      [`body, ${R} .a{fill:red}`, `${R} .a{fill:red}`],
      [`html,body, ${R} .a ,p,${R} .b{fill:red}`, `${R} .a ,${R} .b{fill:red}`],
      [`:is(${R}, body) .a{fill:red}`, ""],
      // garbage between rules is no rule
      [`x;${R} .a{fill:red}`, `${R} .a{fill:red}`],
      [`${R} .a{fill:red} body`, `${R} .a{fill:red}`],
      // inside a rule a nested rule follows the same check
      [`${R} .a{fill:red;&amp; ~ p{display:none}}`, `${R} .a{fill:red;}`],
      // group rules: their rules are checked; a rule list the page reads page-wide goes
      [`@media screen{body{display:none}${R} .a{fill:red}}`, `@media screen{${R} .a{fill:red}}`],
      [`@supports (display:grid){*{color:red}}`, "@supports (display:grid){}"],
      [`@layer a, b;${R} .a{fill:red}`, `${R} .a{fill:red}`],
      [`@layer a{${R} .a{fill:red}}`, ""],
      [`@namespace x "urn:x";${R} .a{fill:red}`, `${R} .a{fill:red}`],
      ['@charset "utf-8";', ""],
      ["@font-face{font-family:f}", ""],
      ["@page{margin:0}", ""],
      ['@property --bg{syntax:"*";inherits:false}', ""],
      ["@container (width > 1px){body{display:none}}", ""],
      ["@scope (body){:scope{display:none}}", ""],
      // keyframe names are the page's: the app's (and its editors') are never taken over
      ["@keyframes pm-fade{to{opacity:0}}", ""],
      ['@keyframes "pm-glow-fade"{to{opacity:0}}', ""],
      ["@-webkit-keyframes PM-fade{to{opacity:0}}", ""],
      ["@keyframes svelte-1x2y3z-chip-in{to{opacity:0}}", ""],
      ["@keyframes cm-blink{50%{opacity:0}}", ""],
      ["@keyframes ProseMirror-cursor-blink{to{opacity:0}}", ""],
      ["@keyframes dash{to{stroke-dashoffset:0;}}", "@keyframes dash{to{stroke-dashoffset:0;}}"],
      [
        "@keyframes edge-animation-frame{from{stroke-dashoffset:0}}",
        "@keyframes edge-animation-frame{from{stroke-dashoffset:0}}",
      ],
    ];
    for (const [written, kept] of cases) expect(sheet(styled(written)), written).toBe(kept);
  });

  it("a drawing's style reaches nothing when its root has no id the page reads as one", () => {
    expect(sheet(onPage(svg('<style>svg .a{fill:red}</style><rect width="1" height="1"/>')))).toBe(
      "",
    );
    // the page's HTML parse lowercases attribute names: two of them would be one id there
    const twice = `<svg id="${ID}" ID="app" xmlns="${SVG_NS}"><style>${R} .a{fill:red}</style><g/></svg>`;
    expect(sheet(onPage(twice, ID))).toBe("");
  });

  it("a style keeps rules only at the root id its caller drew with", () => {
    // a drawing can name its own root after a page element: its rules would reach that element
    const named = `<svg id="app" xmlns="${SVG_NS}"><style>#app p{color:red}</style><g/></svg>`;
    expect(sheet(onPage(named))).toBe("");
    expect(sheet(onPage(named, "pm-mermaid-2-x"))).toBe("");
    expect(sheet(onPage(named, "app"))).toBe("#app p{color:red}");
    const blank = `<svg id="" xmlns="${SVG_NS}"><style>.a{fill:red}</style><g/></svg>`;
    expect(sheet(onPage(blank, ""))).toBe("");
  });

  it("a theme's CSS that would lift the drawing over the window loses its position, nothing else", () => {
    // a Mermaid `themeCSS` of `position:fixed;inset:0` lands on the drawing's root: out of the flow, no box clips it
    const page = onPage(
      scoped(
        `#${ID}{position:fixed;inset:0;background:red}#${ID} .node{fill:#eee;}`,
        "max-width: 80px; position: fixed; top: 0",
      ),
      ID,
    );
    expect(sheet(page)).toBe(`#${ID}{inset:0;background:red}#${ID} .node{fill:#eee;}`);
    expect(page.getAttribute("style")).toBe("max-width: 80px; top: 0");
  });

  it("reads a position the way the page does: any case, comment or escape; only an in-flow one stays", () => {
    const dropped = [
      "POSITION : FIXED !important",
      String.raw`p\osition:fixed`,
      String.raw`posit\69 on: sticky`,
      String.raw`position: fix\65 d`,
      "position:/**/fixed",
      "position: -webkit-sticky",
      "position: absolute",
      "position: relative",
      "position: var(--p)",
      "position: inherit",
      "position: static fixed",
      "position: static !important fixed",
    ];
    for (const written of dropped) {
      const page = onPage(
        drawing(
          `<style>${R} .a{${written}}</style><rect class="a" style="${written}" width="1" height="1"/>`,
        ),
        ID,
      );
      expect(sheet(page), written).toBe(`${R} .a{}`);
      expect(page.querySelector("rect")?.getAttribute("style"), written).toBe("");
    }
    for (const kept of ["position: static", "position:STATIC !important", "position: initial"]) {
      expect(sheet(styled(`${R} .a{${kept}}`)), kept).toBe(`${R} .a{${kept}}`);
    }
  });

  it("a brace in a string, a comment or a function never hides a declaration", () => {
    const cases: [string, string][] = [
      [`${R} .a{content:"}";position:fixed}`, `${R} .a{content:"}";}`],
      [`/* } */${R} .b{ position:fixed; }`, `/* } */${R} .b{ }`],
      [`${R} .c{x:f({;});position:fixed}`, `${R} .c{x:f({;});}`],
      [`@media screen{${R} .d{position:fixed}}`, `@media screen{${R} .d{}}`],
      [`${R} .e{${R} .f{position:sticky}}`, `${R} .e{${R} .f{}}`],
      [`} ${R} .g{position:fixed}`, `} ${R} .g{}`],
    ];
    for (const [written, kept] of cases) expect(sheet(styled(written)), written).toBe(kept);
  });

  it("an attribute keeps only CSS references to parts of the drawing", () => {
    const page = onPage(
      svg(
        `<linearGradient id="g"/><rect fill="url(#g)" filter="url( #g )" stroke="url(https://x.test/p.png)" mask="url('x.svg#m')" clip-path="url(&quot; #g&quot;)" cursor="u&#x5C;72l(x.png), auto" marker-end="URL(pm-asset://x.svg#m)" aria-label="see url(x)" style="fill:url(&quot;#g&quot;); stroke:url(//x.test/a.png); cursor:image-set(&quot;x.png&quot; 1x)" width="1" height="1"/>`,
      ),
    );
    const rect = page.querySelector("rect");
    expect(rect?.getAttributeNames()).toEqual([
      "fill",
      "filter",
      "aria-label",
      "style",
      "width",
      "height",
    ]);
    expect(rect?.getAttribute("style")).toBe('fill:url("#g");');
  });

  it("a style keeps only CSS references to parts of the drawing", () => {
    const cases: [string, string][] = [
      [`${R} .a{fill:url(#g);background:url(pm-asset://x.png)}`, `${R} .a{fill:url(#g);}`],
      [`${R} .a{marker-end:url("#m")}`, `${R} .a{marker-end:url("#m")}`],
      [`${R} .b{background:image-set("https://x.test/a.png" 1x)}`, `${R} .b{}`],
      [`${R} .b{background:-webkit-image-set("a.png" 1x)}`, `${R} .b{}`],
      [`${R} .b{background:src("a.png")}`, `${R} .b{}`],
      [`${R} .b{background:url(" #g")}`, `${R} .b{}`],
      [`${R} .b{background:url(a"b)}`, `${R} .b{}`],
      [String.raw`${R} .b{background:u\rl(//x.test/a)}`, `${R} .b{}`],
      [`${R} .c{--u:url(//x.test/a);fill:red}`, `${R} .c{fill:red}`],
      ["@font-face{font-family:f;src:url(data:font/woff2;base64,AA)}", ""],
      [`@import "x.css";${R} .d{fill:red}`, `${R} .d{fill:red}`],
      [`@import url(x.css);${R} .d{fill:red}`, `${R} .d{fill:red}`],
      [
        `@supports (background:url(x.png)){${R} .e{fill:red}}${R} .f{fill:red}`,
        `${R} .f{fill:red}`,
      ],
      [`${R} .g{fill:red;background:url(https://x.test/a}`, `${R} .g{fill:red;`],
    ];
    for (const [written, kept] of cases) expect(sheet(styled(written)), written).toBe(kept);
  });

  it("a style holding elements goes: the page reads only its own text as CSS", () => {
    const page = onPage(
      svg('<style>.a{fill:red}<g/>.b{fill:url(</style><rect width="1" height="1"/>'),
    );
    expect(tags(page)).toEqual(["rect"]);
  });

  it("leaves a drawing's own CSS byte for byte when nothing in it moves or loads", () => {
    const own = scoped(
      `#${ID}{font-family:"trebuchet ms",verdana;font-size:16px;fill:#333;}#${ID} .marker{fill:#333;stroke:#333;}/* a } note */` +
        `#${ID} .edge{marker-end:url(#${ID}_flowchart-v2-pointEnd);content:"{";}@keyframes dash{to{stroke-dashoffset:0;}}` +
        `@media (prefers-reduced-motion){#${ID} .edge{animation:none;position:static}}`,
    );
    expect(sanitizeSvg(own, ID)).toBe(own);
  });
});

describe("viewBoxWidth", () => {
  it("reads the width of a viewBox", () => {
    expect(viewBoxWidth("0 0 320.5 100")).toBe(320.5);
    expect(viewBoxWidth("0,0,50,20")).toBe(50);
    expect(viewBoxWidth("0 0 0 10")).toBeNull();
    expect(viewBoxWidth("0 0 x 10")).toBeNull();
    expect(viewBoxWidth("1 2 3")).toBeNull();
    expect(viewBoxWidth(null)).toBeNull();
  });
});

describe("diagrams queue", () => {
  beforeEach(() => diagrams.clear());

  it("draws one at a time, in order", async () => {
    const log: string[] = [];
    const job = (name: string, ms: number) => async () => {
      log.push(`start ${name}`);
      await new Promise((r) => setTimeout(r, ms));
      log.push(`end ${name}`);
      return name;
    };
    const [a, b] = await Promise.all([
      diagrams.draw("a", job("a", 20)),
      diagrams.draw("b", job("b", 1)),
    ]);
    expect([a, b]).toEqual(["a", "b"]);
    expect(log).toEqual(["start a", "end a", "start b", "end b"]);
  });

  it("caches results and does not cache failures", async () => {
    let runs = 0;
    const ok = () => {
      runs += 1;
      return Promise.resolve("<svg/>");
    };
    await diagrams.draw("k", ok);
    await diagrams.draw("k", ok);
    expect(runs).toBe(1);
    let failures = 0;
    const failing = () => {
      failures += 1;
      return Promise.reject(new Error("boom"));
    };
    expect(await diagrams.draw("bad", failing)).toBeNull();
    expect(await diagrams.draw("bad", failing)).toBeNull();
    expect(failures).toBe(2);
    expect(await diagrams.draw("none", () => Promise.resolve(null))).toBeNull();
    // a failure does not block the ones queued after it
    expect(await diagrams.draw("after", () => Promise.resolve("x"))).toBe("x");
  });
});
