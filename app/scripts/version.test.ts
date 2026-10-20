import { mkdtempSync, mkdirSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { afterEach, describe, expect, it } from "vitest";
import {
  plan,
  readVersion,
  setByPattern,
  setCargoLockVersions,
  setCargoWorkspaceVersion,
  setJsonVersions,
  setLockfileVersions,
  TARGETS,
} from "./version.mjs";

const CARGO = `[workspace]
members = ["crates/core"]

[workspace.package]
version = "0.5.8"
edition = "2021"

[workspace.dependencies]
foo = { version = "1" }

[package]
name = "permark"
version.workspace = true
`;

const LOCK = `[[package]]
name = "permark"
version = "0.5.8"
dependencies = [
 "permark-core",
]

[[package]]
name = "permark-core"
version = "0.5.8"

[[package]]
name = "permark-thing"
version = "9.9.9"
source = "registry+https://github.com/rust-lang/crates.io-index"
`;

describe("readVersion", () => {
  it("reads x.y.z and pre-releases", () => {
    expect(readVersion('{"version":"1.2.3"}')).toBe("1.2.3");
    expect(readVersion('{"version":"1.2.3-beta.1"}')).toBe("1.2.3-beta.1");
  });

  it("rejects missing, malformed or injected versions", () => {
    for (const bad of [
      "{}",
      '{"version":1}',
      '{"version":"1.2"}',
      '{"version":"v1.2.3"}',
      '{"version":"1.2.3\\"x"}',
      '{"version":"1.2.3\\n"}',
      "[]",
      "null",
    ]) {
      expect(() => readVersion(bad), bad).toThrow();
    }
    expect(() => readVersion("not json")).toThrow();
  });
});

describe("setCargoWorkspaceVersion", () => {
  it("changes only the [workspace.package] version", () => {
    const out = setCargoWorkspaceVersion(CARGO, "1.0.0");
    expect(out).toBe(CARGO.replace('version = "0.5.8"', 'version = "1.0.0"'));
    expect(out).toContain('foo = { version = "1" }');
  });

  it("keeps CRLF line endings and a missing trailing newline", () => {
    const crlf = CARGO.replace(/\n/g, "\r\n").trimEnd();
    const out = setCargoWorkspaceVersion(crlf, "1.0.0");
    expect(out).toBe(crlf.replace('version = "0.5.8"', 'version = "1.0.0"'));
  });

  it("fails without the section or its version", () => {
    expect(() => setCargoWorkspaceVersion('[package]\nversion = "1"\n', "1.0.0")).toThrow();
    expect(() =>
      setCargoWorkspaceVersion("[workspace.package]\nedition = 1\n[x]\n", "1.0.0"),
    ).toThrow();
  });
});

describe("setCargoLockVersions", () => {
  it("changes workspace packages only, never registry packages", () => {
    const out = setCargoLockVersions(LOCK, "1.0.0");
    expect(out.match(/version = "1\.0\.0"/g)).toHaveLength(2);
    expect(out).toContain('version = "9.9.9"');
  });

  it("keeps CRLF", () => {
    const crlf = LOCK.replace(/\n/g, "\r\n");
    expect(setCargoLockVersions(crlf, "1.0.0")).toBe(
      setCargoLockVersions(LOCK, "1.0.0").replace(/\n/g, "\r\n"),
    );
  });

  it("fails when the lock holds no workspace package", () => {
    expect(() =>
      setCargoLockVersions('[[package]]\nname = "serde"\nversion = "1"\n', "1.0.0"),
    ).toThrow();
  });
});

describe("setJsonVersions", () => {
  const json =
    '{\r\n  "metadata": { "version": "0.5.6" },\r\n  "plugins": [{ "version" : "0.5.6" }]\r\n}';

  it("changes exactly the declared versions, keeping formatting", () => {
    const out = setJsonVersions(json, "1.0.0", [
      ["metadata", "version"],
      ["plugins", 0, "version"],
    ]);
    expect(out).toBe(json.replace(/0\.5\.6/g, "1.0.0"));
  });

  it("fails when an undeclared version key appears", () => {
    expect(() => setJsonVersions(json, "1.0.0", [["metadata", "version"]])).toThrow(/found 2/);
  });

  it("fails when a declared path is not where the version is", () => {
    expect(() =>
      setJsonVersions(json, "1.0.0", [
        ["metadata", "version"],
        ["plugins", 1, "version"],
      ]),
    ).toThrow(/plugins\.1\.version/);
  });
});

describe("setLockfileVersions", () => {
  const dependency = '"node_modules/x": {\r\n      "version": "0.5.8"';
  const lock = [
    '{\r\n  "name": "permark",\r\n  "version": "0.5.8",\r\n  "packages": {',
    '    "": {\r\n      "name": "permark",\r\n      "version": "0.5.8"\r\n    },',
    `    ${dependency}\r\n    }\r\n  }\r\n}`,
  ].join("\r\n");

  it("changes the package's own two versions, never a dependency's", () => {
    const out = setLockfileVersions(lock, "1.0.0");
    expect(out.match(/1\.0\.0/g)).toHaveLength(2);
    expect(out).toContain(dependency);
  });

  it("fails without the root entry", () => {
    expect(() =>
      setLockfileVersions('{\n  "name": "a",\n  "version": "1.0.0"\n}', "2.0.0"),
    ).toThrow();
  });
});

describe("setByPattern", () => {
  const docker = setByPattern(/^(ARG FALLBACK_VERSION=)[^\r\n]*()$/gm, "fallback");

  it("replaces the value and keeps CRLF", () => {
    expect(docker("FROM x\r\nARG FALLBACK_VERSION=0.5.6\r\nRUN y\r\n", "1.0.0")).toBe(
      "FROM x\r\nARG FALLBACK_VERSION=1.0.0\r\nRUN y\r\n",
    );
  });

  it("fails when nothing matches", () => {
    expect(() => docker("FROM x\n", "1.0.0")).toThrow(/no fallback/);
  });
});

describe("plan (check + sync)", () => {
  let root = "";
  afterEach(() => {
    if (root) rmSync(root, { recursive: true, force: true });
  });

  function repo(files: Record<string, string>): string {
    root = mkdtempSync(join(tmpdir(), "permark version ü "));
    for (const [path, text] of Object.entries(files)) {
      mkdirSync(dirname(join(root, path)), { recursive: true });
      writeFileSync(join(root, path), text);
    }
    return root;
  }

  it("reports only the files that drift (paths with spaces + Unicode)", () => {
    const targets = TARGETS.filter((t) => t.path === "Dockerfile" || t.path === "llms-install.md");
    const dir = repo({
      Dockerfile: "ARG FALLBACK_VERSION=1.0.0\n",
      "llms-install.md": "get PerMark_0.9.0_amd64.deb\n",
    });
    expect(plan(dir, "1.0.0", targets)).toEqual([
      { path: "llms-install.md", text: "get PerMark_1.0.0_amd64.deb\n" },
    ]);
  });

  it("names the file when a target is malformed", () => {
    const targets = TARGETS.filter((t) => t.path === "gemini-extension.json");
    const dir = repo({ "gemini-extension.json": '{ "name": "x" }' });
    expect(() => plan(dir, "1.0.0", targets)).toThrow(/^gemini-extension\.json: /);
  });
});
