import { describe, expect, it } from "vitest";
import { relativeTime } from "./time";

const NOW = Date.UTC(2026, 9, 19, 12, 0, 0);
const MIN = 60_000;
const HOUR = 60 * MIN;
const DAY = 24 * HOUR;

describe("relativeTime", () => {
  it("says it the way people do, in the language asked", () => {
    expect(relativeTime(NOW - 10_000, NOW, "en")).toBe("now");
    expect(relativeTime(NOW - 5 * MIN, NOW, "en")).toBe("5 minutes ago");
    expect(relativeTime(NOW - 3 * HOUR, NOW, "en")).toBe("3 hours ago");
    expect(relativeTime(NOW - DAY, NOW, "en")).toBe("yesterday");
    expect(relativeTime(NOW - 4 * DAY, NOW, "en")).toBe("4 days ago");
    expect(relativeTime(NOW - 90 * DAY, NOW, "en")).toBe("3 months ago");
    expect(relativeTime(NOW - 800 * DAY, NOW, "en")).toBe("2 years ago");
    expect(relativeTime(NOW - 3 * HOUR, NOW, "de")).toBe("vor 3 Stunden");
    expect(relativeTime(NOW - DAY, NOW, "pt-BR")).toBe("ontem");
  });

  it("a moment ahead of now (a clock that differs) reads as now, and nothing throws on odd numbers", () => {
    expect(relativeTime(NOW + DAY, NOW, "en")).toBe("now");
    expect(relativeTime(0, NOW, "en")).toMatch(/years ago$/);
    expect(relativeTime(NOW, NOW, "zh-CN")).not.toBe("");
  });
});
