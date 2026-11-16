// The attack corpus (`corpus/security/`) in every mode: nothing may run, load a page or call out. The guards of every
// deep page (requests off this machine, CSP violations, dialogs, page errors, the canary) decide; this spec also
// hovers and clicks every link of the reading view and checks the page stayed where it was.

import { expect, expectClean, test } from "./fixtures.ts";
import { corpusPages } from "./settings.ts";

const ATTACKS = corpusPages().filter((view) => view.path.includes("/security/"));

test.describe("security", () => {
  test.skip(ATTACKS.length === 0, "no attack pages in this corpus (corpus/security/)");

  for (const view of ATTACKS) {
    test(view.path, async ({ app }) => {
      const page = app.page;
      const where = (): string => {
        const url = new URL(page.url());
        return url.origin + url.pathname;
      };
      const start = where();
      await app.show(view.path);
      await app.drawAll();

      // a link that kept a script URL runs on click; web links go to the system (mock), anchors scroll
      const links = app.body.locator("a[href]").filter({ visible: true });
      const count = await links.count();
      expect(count, "links in the attack page").toBeGreaterThan(0);
      for (let at = 0; at < count; at++) {
        const link = links.nth(at);
        await link.scrollIntoViewIfNeeded();
        await link.hover();
        await link.click({ modifiers: [], noWaitAfter: true });
        await page.keyboard.press("Escape");
      }
      await app.drawn();
      expect(where(), "the page stays the app").toBe(start);

      await app.checkpoint("view + links");
      for (const mode of ["edit", "source", "view"] as const) {
        await app.top();
        await app.mode(mode);
        await app.drawAll();
        await app.checkpoint(mode);
      }

      expectClean(app.labeled);
    });
  }
});
