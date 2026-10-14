import { readFileSync } from "node:fs";
import { describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  convertFileSrc: vi.fn(
    (path: string, scheme: string) => `${scheme}://localhost/${encodeURIComponent(path)}`,
  ),
}));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/plugin-opener", () => ({ openUrl: vi.fn() }));

import { assetUrl } from "./platform";

const read = (path: string): string => readFileSync(path, "utf8");

/** The scheme the backend registers for the images of the open folder. */
const rustScheme = /ASSET_SCHEME: &str = "([a-z]+)"/.exec(read("src-tauri/src/commands.rs"))?.[1];

describe("image URLs", () => {
  it("point at the backend's image scheme, with the vault path as one encoded segment", () => {
    expect(rustScheme).toBeTruthy();
    expect(assetUrl("docs/img/a b.png")).toBe(`${rustScheme}://localhost/docs%2Fimg%2Fa%20b.png`);
  });
});

describe("security configuration", () => {
  const config = JSON.parse(read("src-tauri/tauri.conf.json")) as {
    app: { security: { csp: string; assetProtocol?: unknown } };
    plugins?: unknown;
  };
  const csp = config.app.security.csp;
  const directive = (name: string): string[] =>
    (
      csp
        .split(";")
        .map((d) => d.trim().split(/\s+/))
        .find(([n]) => n === name) ?? []
    ).slice(1);

  it("lets the document view load the backend's images, data images and remote https images", () => {
    const img = directive("img-src");
    expect(img).toEqual(
      expect.arrayContaining([
        `${rustScheme}:`,
        `http://${rustScheme}.localhost`,
        "data:",
        "https:",
      ]),
    );
    expect(img).not.toContain("http:");
    expect(img).not.toContain("*");
  });

  it("does not use the asset protocol, whose scope can only grow", () => {
    expect(config.app.security.assetProtocol).toBeUndefined();
    const sources = csp.split(/[\s;]+/);
    expect(sources).not.toContain("asset:");
    expect(sources).not.toContain("http://asset.localhost");
    expect(read("src-tauri/Cargo.toml")).not.toContain("protocol-asset");
  });

  it("does not let the webview open arbitrary files", () => {
    const capability = JSON.parse(read("src-tauri/capabilities/default.json")) as {
      permissions: (string | { identifier: string })[];
    };
    const granted = capability.permissions.map((p) => (typeof p === "string" ? p : p.identifier));
    expect(granted.filter((p) => /^opener:.*(open-path|reveal)/.test(p))).toEqual([]);
    expect(granted).toEqual(expect.arrayContaining(["opener:allow-open-url"]));
  });
});
