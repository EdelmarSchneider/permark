// Render goldens of every corpus page. Text goldens (in the repository): the renderer's blocks and a digest of the
// reading view after everything is drawn. Pixel baselines (outside it): the first screen and each embed-like element
// in View, light and dark at 1200 px (tables and code also at 700 px), and the first screen of Source and Edit.

import type { Locator } from "@playwright/test";
import { blockGolden, domDigest, slugOf } from "./digest.ts";
import { expect, expectClean, expectGolden, test, type DeepApp } from "./fixtures.ts";
import { corpusPages, PIXELS, QUICK } from "./settings.ts";

const THEMES = QUICK ? (["light"] as const) : (["light", "dark"] as const);

/** Elements shot one by one in View (visible ones: a folded callout's insides are not on screen). */
const ELEMENTS: readonly (readonly [string, string])[] = [
  ["diagram", ".md-body .code-block.diagram"],
  ["math", ".md-body .katex-display"],
  ["table", ".md-body .table-wrap"],
  ["callout", ".md-body .markdown-alert"],
  ["code", ".md-body .code-block:not(.diagram)"],
  ["card", ".reading section.card"],
];
const ALL_KINDS = ELEMENTS.map(([kind]) => kind);

/** Elements shot again in a narrow window (they scroll or wrap there). */
const NARROW: readonly string[] = ["table", "code"];
const NARROW_WIDTH = 700;

const PAGES = corpusPages();

async function shot(target: Locator, slug: string, name: string): Promise<void> {
  if (!PIXELS) return;
  await expect.soft(target).toHaveScreenshot([slug, `${name}.png`]);
}

async function shootElements(
  app: DeepApp,
  slug: string,
  prefix: string,
  kinds: readonly string[],
): Promise<void> {
  if (!PIXELS) return;
  for (const [kind, selector] of ELEMENTS) {
    if (!kinds.includes(kind)) continue;
    const all = app.page.locator(selector).filter({ visible: true });
    const count = await all.count();
    for (let at = 0; at < count; at++) {
      const element = all.nth(at);
      await element.scrollIntoViewIfNeeded();
      await app.drawn();
      await shot(element, slug, `${prefix}-${kind}-${at + 1}`);
    }
  }
}

test.describe("blocks", () => {
  for (const view of PAGES) {
    test(view.path, async () => {
      await expectGolden(`${slugOf(view.path)}.blocks.txt`, blockGolden(view));
    });
  }
});

for (const theme of THEMES) {
  test.describe(theme, () => {
    test.use({ colorScheme: theme });

    for (const view of PAGES) {
      test(view.path, async ({ app }) => {
        const slug = slugOf(view.path);
        const page = app.page;
        await app.show(view.path);
        await expect(page.locator("html")).toHaveAttribute("data-theme", theme);
        await app.drawAll();

        if (theme === "light") {
          const origin = new URL(page.url()).origin;
          const digest = await page.evaluate(domDigest, { selector: ".reading", origin });
          expect(digest, "reading view").not.toBeNull();
          await expectGolden(`${slug}.dom.txt`, digest ?? "");
        }

        await app.checkpoint("view");
        await shot(app.pane, slug, `${theme}-view-first`);
        await shootElements(app, slug, `${theme}-view`, ALL_KINDS);
        if (!QUICK) {
          const wide = page.viewportSize() ?? { width: 1200, height: 800 };
          await page.setViewportSize({ width: NARROW_WIDTH, height: wide.height });
          await app.drawn();
          await shootElements(app, slug, `${theme}-view${NARROW_WIDTH}`, NARROW);
          await page.setViewportSize(wide);

          await app.top();
          await app.mode("source");
          await app.checkpoint("source");
          await shot(app.pane, slug, `${theme}-source-first`);
          await app.mode("edit");
          await app.drawn();
          await app.checkpoint("edit");
          await shot(app.pane, slug, `${theme}-edit-first`);
          await app.mode("view");
        }

        await app.checkpoint("view");
        expectClean(app.labeled);
      });
    }
  });
}

/** A corpus page whose link target is deleted at run time (its goldens and baselines stay as they are). */
const MISSING = {
  page: "corpus/gfm-blocks.md",
  href: "gfm-tables.md",
  gone: "corpus/gfm-tables.md",
} as const;

// Edit draws View's missing-link look (class + "not found" title) only while a plugin that checks links is on. The
// switch flips with no edit in between, so only a repaint of the editor's inline looks can take the marks away and
// bring them back.
test.describe("missing-link marks in Edit", () => {
  test.skip(QUICK, "a quick run checks View only");
  test.skip(
    !PAGES.some((view) => view.path === MISSING.page),
    `${MISSING.page} is not in this corpus`,
  );

  test(`${MISSING.page} follows the broken-links plugin`, async ({ app }) => {
    await app.deleteFiles([MISSING.gone]);
    await app.show(MISSING.page);
    await app.mode("edit");
    await app.drawn();
    const link = app.page.locator(`.pm-editor a[href="${MISSING.href}"]`);
    // the title sits on the text inside the link, split where another feature marks a word (spelling); the run's
    // locale is English (playwright.config.ts)
    const titled = link.locator(`[title="Not found: ${MISSING.gone}"]`);
    const marked = /(^|\s)is-missing(\s|$)/;

    await expect(link).toHaveCount(1);
    await expect(link).toHaveClass(marked);
    await expect(titled.first()).toBeAttached();

    await app.setPlugin("broken-links", false);
    await expect(link).toHaveCount(1);
    await expect(link).not.toHaveClass(marked);
    await expect(titled).toHaveCount(0);

    await app.setPlugin("broken-links", true);
    await expect(link).toHaveClass(marked);
    await expect(titled.first()).toBeAttached();

    await app.checkpoint("edit");
    expectClean(app.labeled);
  });
});
