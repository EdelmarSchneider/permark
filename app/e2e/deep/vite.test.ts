import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { afterAll, describe, expect, it } from "vitest";
import { FILES_ROUTE } from "./paths.ts";
import { appCsp, corpusFile } from "./vite.ts";

describe("corpusFile", () => {
  const home = mkdtempSync(path.join(tmpdir(), "permark-deep-test-"));
  const project = path.join(home, "project");
  mkdirSync(path.join(project, "demo", "a b"), { recursive: true });
  writeFileSync(path.join(project, "demo", "a b", "x.svg"), "<svg/>");
  writeFileSync(path.join(home, "outside.txt"), "");
  afterAll(() => rmSync(home, { recursive: true, force: true }));

  it("finds a file of the project from its encoded URL, query and fragment aside", () => {
    expect(corpusFile(project, `${FILES_ROUTE}demo/a%20b/x.svg?t=1#f`)).toBe(
      path.join(project, "demo", "a b", "x.svg"),
    );
  });

  it("serves nothing outside the project, no folder and no missing file", () => {
    for (const url of [
      "../outside.txt",
      "..%2Foutside.txt",
      "demo/..%2F..%2Foutside.txt",
      "%2e%2e/outside.txt",
      "demo",
      "",
      "demo/missing.svg",
      "%E0%A4%A",
    ]) {
      expect(corpusFile(project, FILES_ROUTE + url), url).toBeNull();
    }
  });
});

describe("appCsp", () => {
  it("is the app's own policy: no inline script, nothing loaded from the web", () => {
    const csp = appCsp();
    expect(csp).toContain("default-src 'self'");
    const script = csp
      .split(";")
      .map((d) => d.trim().split(/\s+/))
      .find((d) => d[0] === "script-src");
    expect(script).toContain("'self'");
    expect(script).not.toContain("'unsafe-eval'");
    expect(script).not.toContain("'unsafe-inline'");
    expect(csp).toContain("object-src 'none'");
  });
});
