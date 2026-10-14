import { describe, expect, it } from "vitest";
import { ago, baselineLabel, clock, relativeTime, savedLabel } from "./time";

const NOW = new Date(2026, 9, 13, 12, 0, 0).getTime();
const SEC = 1000;
const MIN = 60 * SEC;
const HOUR = 60 * MIN;

describe("relativeTime", () => {
  it("is compact and rounds down", () => {
    expect(relativeTime(NOW - 2 * SEC, NOW)).toBe("just now");
    expect(relativeTime(NOW - 12 * SEC, NOW)).toBe("12s");
    expect(relativeTime(NOW - 4 * MIN - 30 * SEC, NOW)).toBe("4m");
    expect(relativeTime(NOW - 3 * HOUR, NOW)).toBe("3h");
    expect(relativeTime(NOW - 50 * HOUR, NOW)).toBe("2d");
  });

  it("treats a time in the future as now", () => {
    expect(relativeTime(NOW + 10 * SEC, NOW)).toBe("just now");
  });
});

describe("ago", () => {
  it("spells the unit out", () => {
    expect(ago(NOW - 1 * MIN, NOW)).toBe("1 minute ago");
    expect(ago(NOW - 12 * SEC, NOW)).toBe("12 seconds ago");
    expect(ago(NOW - 2 * HOUR, NOW)).toBe("2 hours ago");
  });
});

describe("savedLabel", () => {
  it("says 'saved just now' and then the compact age", () => {
    expect(savedLabel(NOW - SEC, NOW)).toBe("saved just now");
    expect(savedLabel(NOW - 12 * SEC, NOW)).toBe("saved 12s ago");
  });
});

describe("clock and baselineLabel", () => {
  it("formats local time as HH:MM", () => {
    expect(clock(new Date(2026, 9, 13, 9, 5).getTime())).toBe("09:05");
  });

  it("names the weekday within the last week", () => {
    const monday = new Date(2026, 9, 12, 18, 40).getTime(); // Mon Oct 12 2026
    expect(baselineLabel(monday, NOW)).toBe("Mon 18:40");
  });

  it("uses the date for older baselines", () => {
    const old = new Date(2026, 9, 1, 8, 15).getTime();
    expect(baselineLabel(old, NOW)).toBe("Oct 1, 08:15");
  });
});
