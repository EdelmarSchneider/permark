import { afterEach, describe, expect, it } from "vitest";
import { language } from "./i18n";
import { ago, baselineLabel, clock, dateTime, relativeTime, savedLabel } from "./time";

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

describe("in another language", () => {
  afterEach(() => {
    language.preference = "en";
  });

  it("words the age with the language's own grammar", () => {
    language.preference = "pt-BR";
    expect(ago(NOW - 12 * SEC, NOW)).toBe("há 12 segundos");
    expect(ago(NOW - 1 * MIN, NOW)).toBe("há 1 minuto");
    language.preference = "es";
    expect(ago(NOW - 2 * HOUR, NOW)).toBe("hace 2 horas");
    language.preference = "de";
    expect(ago(NOW - 2 * HOUR, NOW)).toBe("vor 2 Stunden");
    expect(ago(NOW - 1 * MIN, NOW)).toBe("vor 1 Minute");
    language.preference = "zh-CN";
    expect(ago(NOW - 2 * HOUR, NOW)).toBe("2 小时前");
    expect(ago(NOW - 12 * SEC, NOW)).toBe("12 秒钟前");
    expect(ago(NOW - 17 * MIN, NOW)).toBe("17 分钟前");
    expect(ago(NOW - 50 * HOUR, NOW)).toBe("2 天前");
    expect(savedLabel(NOW - 4 * MIN, NOW)).toBe("4 分钟前保存");
    language.preference = "it";
    expect(ago(NOW - 2 * HOUR, NOW)).toBe("2 ore fa");
    expect(ago(NOW - 1 * MIN, NOW)).toBe("1 minuto fa");
    expect(ago(NOW - 12 * SEC, NOW)).toBe("12 secondi fa");
    expect(savedLabel(NOW - 4 * MIN, NOW)).toBe("salvato 4 min fa");
  });

  it("says 'just now' in the language", () => {
    language.preference = "de";
    expect(ago(NOW - SEC, NOW)).toBe("eben");
    expect(savedLabel(NOW - SEC, NOW)).toBe("eben geändert");
    expect(savedLabel(NOW - 12 * SEC, NOW)).toBe("vor 12 s geändert");
    language.preference = "it";
    expect(ago(NOW - SEC, NOW)).toBe("proprio ora");
    expect(savedLabel(NOW - SEC, NOW)).toBe("salvato proprio ora");
    expect(savedLabel(NOW - 12 * SEC, NOW)).toBe("salvato 12 s fa");
  });

  it("keeps the 24-hour clock and names the day in the language", () => {
    language.preference = "de";
    expect(clock(new Date(2026, 9, 13, 18, 5).getTime())).toBe("18:05");
    const monday = new Date(2026, 9, 12, 18, 40).getTime();
    expect(baselineLabel(monday, NOW)).toMatch(/^Mo\.? 18:40$/);
    language.preference = "pt-BR";
    expect(baselineLabel(monday, NOW)).toMatch(/^seg\.? 18:40$/);
    language.preference = "zh-CN";
    expect(baselineLabel(monday, NOW)).toBe("周一 18:40");
    language.preference = "it";
    expect(baselineLabel(monday, NOW)).toBe("lun 18:40");
  });

  it("puts the date and the time together the way each language writes them", () => {
    const old = new Date(2026, 9, 1, 8, 15).getTime();
    language.preference = "pt-BR";
    expect(baselineLabel(old, NOW)).toMatch(/^1 de out\.?, 08:15$/);
    language.preference = "es";
    expect(baselineLabel(old, NOW)).toMatch(/^1 oct\.?, 08:15$/);
    language.preference = "de";
    expect(baselineLabel(old, NOW)).toMatch(/^1\. Okt\.?, 08:15$/);
    // Chinese writes no comma between the date and the time
    language.preference = "zh-CN";
    expect(baselineLabel(old, NOW)).toBe("10月1日 08:15");
    language.preference = "it";
    expect(baselineLabel(old, NOW)).toBe("1 ott, 08:15");
  });
});

describe("dateTime", () => {
  afterEach(() => {
    language.preference = "en";
  });

  const lastYear = new Date(2025, 11, 31, 0, 5).getTime();

  it("is the date and the 24-hour time, without the year unless asked", () => {
    const morning = new Date(2026, 9, 1, 8, 15).getTime();
    expect(dateTime(morning)).toBe("Oct 1, 08:15");
    expect(dateTime(lastYear)).toBe("Dec 31, 00:05");
    expect(dateTime(lastYear, true)).toBe("Dec 31, 2025, 00:05");
  });

  it("follows the language, the year included", () => {
    language.preference = "de";
    expect(dateTime(lastYear, true)).toMatch(/^31\. Dez\.? 2025,? 00:05$/);
    language.preference = "zh-CN";
    expect(dateTime(lastYear, true)).toBe("2025年12月31日 00:05");
    language.preference = "it";
    expect(dateTime(lastYear, true)).toBe("31 dic 2025, 00:05");
  });
});
