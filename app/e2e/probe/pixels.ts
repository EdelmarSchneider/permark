// Scroll bar thumbs, by their pixels: the scroll bar's lane is cut out of a screenshot and read in a blank page (the
// app's own page is never touched). Spec: probe.md §7 `scrollers`.

import type { BrowserContext, Page } from "@playwright/test";
import type { Thumb } from "./facts.ts";
import type { ScrollerFact } from "./page.ts";

/** Columns of the lane that differ from its most common color (difference > `tolerance`, summed RGB). */
export function thumbColumns(
  data: ArrayLike<number>,
  width: number,
  height: number,
  tolerance = 24,
): number {
  const counts = new Map<number, number>();
  for (let i = 0; i < width * height; i++) {
    const key = ((data[i * 4] ?? 0) << 16) | ((data[i * 4 + 1] ?? 0) << 8) | (data[i * 4 + 2] ?? 0);
    counts.set(key, (counts.get(key) ?? 0) + 1);
  }
  let bg = 0;
  let most = -1;
  for (const [key, n] of counts) {
    if (n > most) {
      most = n;
      bg = key;
    }
  }
  const [br, bgG, bb] = [(bg >> 16) & 255, (bg >> 8) & 255, bg & 255];
  let columns = 0;
  for (let x = 0; x < width; x++) {
    for (let y = 0; y < height; y++) {
      const i = (y * width + x) * 4;
      const d =
        Math.abs((data[i] ?? 0) - br) +
        Math.abs((data[i + 1] ?? 0) - bgG) +
        Math.abs((data[i + 2] ?? 0) - bb);
      if (d > tolerance) {
        columns += 1;
        break;
      }
    }
  }
  return columns;
}

/** The thumb of each scroller with a lane (overlay scroll bars: none to read, skipped). */
export async function thumbs(
  page: Page,
  context: BrowserContext,
  scrollers: ScrollerFact[],
): Promise<Thumb[]> {
  const lanes = scrollers.filter((s) => s.lane !== null && s.lane.w > 0 && s.lane.h > 0);
  if (lanes.length === 0) return [];
  const reader = await context.newPage();
  try {
    const out: Thumb[] = [];
    for (const s of lanes) {
      const lane = s.lane;
      if (lane === null) continue;
      const png = await page.screenshot({
        clip: { x: lane.x, y: lane.y, width: lane.w, height: lane.h },
      });
      const pixels = await reader.evaluate(async (b64: string) => {
        const img = new Image();
        img.src = `data:image/png;base64,${b64}`;
        await img.decode();
        const canvas = document.createElement("canvas");
        canvas.width = img.naturalWidth;
        canvas.height = img.naturalHeight;
        const ctx = canvas.getContext("2d");
        if (!ctx) return null;
        ctx.drawImage(img, 0, 0);
        const data = ctx.getImageData(0, 0, canvas.width, canvas.height).data;
        return { w: canvas.width, h: canvas.height, data: Array.from(data) };
      }, png.toString("base64"));
      if (pixels === null) continue;
      const columns = thumbColumns(pixels.data, pixels.w, pixels.h);
      // device pixels -> CSS pixels
      const scale = pixels.w / lane.w;
      out.push({
        selector: s.selector,
        visible: columns > 0,
        width: Math.round(columns / (scale || 1)),
      });
    }
    return out;
  } finally {
    await reader.close();
  }
}
