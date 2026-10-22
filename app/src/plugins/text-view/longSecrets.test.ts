import { describe, expect, it } from "vitest";
import { findCodeSecrets } from "../../core/config/secrets";
import { longLineSpans, type WindowCache } from "./longSecrets";

const SECRET = "abc123secretvalue";
const pad = (n: number): string => "x ".repeat(n / 2);

describe("longLineSpans", () => {
  it("finds what a whole-line scan finds, by windows", () => {
    const line = `API_TOKEN=${SECRET} ${pad(9_000)}password=${SECRET} ${pad(9_000)}`;
    const cache: WindowCache = new Map();
    const whole = findCodeSecrets(line);
    expect(whole.length).toBe(2);
    expect(longLineSpans(line, 0, line.length, cache)).toEqual(whole);
  });

  it("a pair across a window edge stays one span", () => {
    for (const at of [2_040, 2_046, 2_048, 2_050, 4_090]) {
      const line = `${pad(at)}password=${SECRET} ${pad(8_000)}`;
      const spans = longLineSpans(line, 0, line.length, new Map());
      expect(spans.map((s) => line.slice(s.from, s.to))).toEqual([SECRET]);
    }
  });

  it("a value longer than the overlap is masked to its end", () => {
    const value = "Q".repeat(3_000);
    const line = `${pad(1_900)}password=${value} ${pad(8_000)}`;
    const spans = longLineSpans(line, 0, line.length, new Map());
    expect(spans.map((s) => line.slice(s.from, s.to))).toEqual([value]);
    // a range inside the value, from the window after it starts
    expect(longLineSpans(line, 4_000, 4_100, new Map()).length).toBe(1);
  });

  it("scans only the windows a range needs, once", () => {
    const line = `${pad(300_000)}password=${SECRET} ${pad(300_000)}`;
    const cache: WindowCache = new Map();
    const spans = longLineSpans(line, 300_000, 300_200, cache);
    expect(spans.map((s) => line.slice(s.from, s.to))).toEqual([SECRET]);
    expect(cache.size).toBeLessThanOrEqual(3);
    expect(longLineSpans(line, 0, 100, cache)).toEqual([]);
  });
});
