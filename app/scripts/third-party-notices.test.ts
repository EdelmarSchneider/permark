import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  bundledKeys,
  fenceFor,
  groupNotices,
  licenseOf,
  licenseText,
  nameOfKey,
  npmNotices,
  renderGroup,
  renderNotices,
  resolveDependency,
} from "./third-party-notices.mjs";

const pkg = (extra: Record<string, unknown> = {}) => ({ version: "1.0.0", ...extra });

// app -> a -> b (hoisted); a -> c (nested under a, another version of c also hoisted); tool = dev only
const LOCK = {
  packages: {
    "": { dependencies: { a: "1" } },
    "node_modules/a": pkg({ dependencies: { b: "1", c: "2" } }),
    "node_modules/a/node_modules/c": pkg({ version: "2.0.0" }),
    "node_modules/b": pkg({ peerDependencies: { p: "1" }, optionalDependencies: { o: "1" } }),
    "node_modules/c": pkg(),
    "node_modules/p": pkg(),
    "node_modules/o": pkg({ optional: true }),
    "node_modules/tool": pkg({ dependencies: { a: "1" } }),
  },
};

describe("resolveDependency", () => {
  it("takes the nearest node_modules folder, like Node", () => {
    expect(resolveDependency(LOCK.packages, "node_modules/a", "c")).toBe(
      "node_modules/a/node_modules/c",
    );
    expect(resolveDependency(LOCK.packages, "node_modules/b", "c")).toBe("node_modules/c");
    expect(resolveDependency(LOCK.packages, "", "a")).toBe("node_modules/a");
  });
  it("finds a package from inside a nested one, and null when nothing has it", () => {
    expect(resolveDependency(LOCK.packages, "node_modules/a/node_modules/c", "b")).toBe(
      "node_modules/b",
    );
    expect(resolveDependency(LOCK.packages, "node_modules/a", "nope")).toBeNull();
  });
});

describe("bundledKeys", () => {
  it("lists the roots and everything they need, peer and optional ones too, not tooling", () => {
    expect(bundledKeys(LOCK, ["a"])).toEqual([
      "node_modules/a",
      "node_modules/a/node_modules/c",
      "node_modules/b",
      "node_modules/o",
      "node_modules/p",
    ]);
  });
  it("survives a cycle", () => {
    const cyclic = {
      packages: {
        "": {},
        "node_modules/x": pkg({ dependencies: { y: "1" } }),
        "node_modules/y": pkg({ dependencies: { x: "1" } }),
      },
    };
    expect(bundledKeys(cyclic, ["x"])).toEqual(["node_modules/x", "node_modules/y"]);
  });
  it("refuses a root the lockfile does not have", () => {
    expect(() => bundledKeys(LOCK, ["missing"])).toThrow(/missing/);
  });
});

describe("nameOfKey + licenseOf", () => {
  it("names scoped and nested packages", () => {
    expect(nameOfKey("node_modules/@scope/pkg")).toBe("@scope/pkg");
    expect(nameOfKey("node_modules/a/node_modules/@s/b")).toBe("@s/b");
  });
  it("reads the license field in its old and new shapes", () => {
    expect(licenseOf({ license: "MIT" })).toBe("MIT");
    expect(licenseOf({ license: { type: "ISC" } })).toBe("ISC");
    expect(licenseOf({ licenses: [{ type: "MIT" }, { type: "Apache-2.0" }] })).toBe(
      "MIT OR Apache-2.0",
    );
    expect(licenseOf({})).toBe("");
    expect(licenseOf(null)).toBe("");
    expect(licenseOf({ license: 7 })).toBe("");
  });
});

describe("with packages on disk", () => {
  const dirs: string[] = [];
  afterEach(() => {
    for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true });
  });
  function modules(layout: Record<string, Record<string, string>>): string {
    const root = mkdtempSync(join(tmpdir(), "permark-notices-test-"));
    dirs.push(root);
    for (const [key, files] of Object.entries(layout)) {
      const dir = join(root, key);
      mkdirSync(dir, { recursive: true });
      for (const [name, content] of Object.entries(files)) writeFileSync(join(dir, name), content);
    }
    return root;
  }

  it("reads license + notice files only, with LF line ends, in name order", () => {
    const root = modules({
      p: {
        LICENSE: "MIT text\r\nline two\r\n",
        "NOTICE.md": "notice",
        "license-apache.txt": "apache",
        "README.md": "readme",
        "licensed-things.js": "code",
      },
    });
    expect(licenseText(join(root, "p"))).toBe("MIT text\nline two\n\nnotice\n\napache");
  });

  it("gives an empty text when a package has no license file", () => {
    const root = modules({ p: { "README.md": "x" } });
    expect(licenseText(join(root, "p"))).toBe("");
  });

  it("collects the bundled packages, skips an uninstalled optional one, warns about a missing file", () => {
    const root = modules({
      a: { "package.json": '{"name":"a","version":"1.0.0","license":"MIT"}', LICENSE: "text a" },
      "a/node_modules/c": {
        "package.json": '{"name":"c","version":"2.0.0","license":"MIT"}',
        LICENSE: "text a",
      },
      b: { "package.json": '{"name":"b","version":"1.0.0","license":"ISC"}' },
      c: { "package.json": '{"name":"c","version":"1.0.0","license":"MIT"}', LICENSE: "text a" },
      p: { "package.json": '{"name":"p","version":"1.0.0","license":"MIT"}', LICENSE: "text p" },
    });
    const { notices, warnings } = npmNotices(LOCK, ["a"], root);
    expect(notices.map((n) => `${n.name} ${n.version}`)).toEqual([
      "a 1.0.0",
      "c 2.0.0",
      "b 1.0.0",
      "p 1.0.0",
    ]);
    expect(warnings).toEqual(["b 1.0.0: no license file (declares ISC)"]);
  });

  it("fails on a package that says nothing about its license, and on a broken install", () => {
    const silent = modules({ a: { "package.json": '{"name":"a","version":"1.0.0"}' } });
    expect(() =>
      npmNotices({ packages: { "": {}, "node_modules/a": pkg() } }, ["a"], silent),
    ).toThrow(/no license/);
    const empty = modules({});
    expect(() =>
      npmNotices({ packages: { "": {}, "node_modules/a": pkg() } }, ["a"], empty),
    ).toThrow(/not installed/);
  });
});

describe("groupNotices", () => {
  it("groups equal texts whatever their spacing, naming each package once", () => {
    const groups = groupNotices([
      { name: "b", version: "1.0.0", license: "MIT", text: "Permission  is\ngranted" },
      { name: "a", version: "2.0.0", license: "MIT", text: "Permission is granted\n" },
      { name: "c", version: "1.0.0", license: "MIT", text: "Permission is granted by C" },
    ]);
    expect(groups).toHaveLength(2);
    expect(groups[0]?.users).toEqual(["a 2.0.0", "b 1.0.0"]);
    expect(groups[1]?.users).toEqual(["c 1.0.0"]);
  });
  it("keeps packages without a text together by their declared license", () => {
    const groups = groupNotices([
      { name: "a", version: "1.0.0", license: "MIT", text: "" },
      { name: "b", version: "1.0.0", license: "MIT", text: "" },
      { name: "c", version: "1.0.0", license: "ISC", text: "" },
    ]);
    expect(groups.map((g) => g.users)).toEqual([["c 1.0.0"], ["a 1.0.0", "b 1.0.0"]]);
  });
});

describe("rendering", () => {
  it("fences a text that holds a fence of its own with a longer one", () => {
    expect(fenceFor("plain")).toBe("~~~~");
    expect(fenceFor("a ~~~~~~ b")).toBe("~~~~~~~");
    const group = { licenses: ["MIT"], text: "x\n~~~~\ny", users: ["a 1.0.0"] };
    const out = renderGroup(group);
    expect(out).toContain("~~~~~text\nx\n~~~~\ny\n~~~~~\n");
    expect(out.startsWith("### MIT\n\nUsed by: a 1.0.0\n")).toBe(true);
  });
  it("says so when a group has no text", () => {
    const out = renderGroup({ licenses: ["MIT"], text: "", users: ["a 1.0.0"] });
    expect(out).toContain("ship no license file");
    expect(out).not.toContain("~~~~");
  });
  it("puts the three sources under their own headings", () => {
    const file = renderNotices({
      rust: "### MIT License\n\nUsed by: x 1.0.0\n",
      npm: [{ licenses: ["MIT"], text: "t", users: ["y 1.0.0"] }],
      libgit2: { version: "1.9.7", text: "GPL text\n" },
    });
    const headings = file.split("\n").filter((line) => line.startsWith("## "));
    expect(headings).toEqual([
      "## Rust crates (compiled into the program)",
      "## npm packages (bundled in the interface, fonts included)",
      "## libgit2 1.9.7 (C library compiled into the program)",
    ]);
    expect(file.endsWith("~~~~\n")).toBe(true);
  });
});
