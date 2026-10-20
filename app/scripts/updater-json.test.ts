import { describe, expect, it } from "vitest";
import { parseRelease, updaterJson } from "./updater-json.mjs";

// Asset names of the 0.6.1 release (unchanged since 0.6.0; `distribution.md` § Release pipeline).
const ASSETS = [
  "latest.json",
  "PerMark-0.6.1-1.x86_64.rpm",
  "PerMark-0.6.1-1.x86_64.rpm.sig",
  "PerMark_0.6.1_amd64.AppImage",
  "PerMark_0.6.1_amd64.AppImage.sig",
  "PerMark_0.6.1_amd64.deb",
  "PerMark_0.6.1_amd64.deb.sig",
  "PerMark_0.6.1_universal.dmg",
  "PerMark_0.6.1_x64-setup.exe",
  "PerMark_0.6.1_x64-setup.exe.sig",
  "PerMark_0.6.1_x64_en-US.msi",
  "PerMark_0.6.1_x64_en-US.msi.sig",
  "PerMark_universal.app.tar.gz",
  "PerMark_universal.app.tar.gz.sig",
];

const BASE = "https://github.com/EdelmarSchneider/permark/releases/download/v0.6.1/";

function build(
  assets: readonly string[],
  overrides: Partial<Parameters<typeof updaterJson>[0]> = {},
) {
  return updaterJson({
    version: "0.6.1",
    notes: "**PerMark** notes\n\n### Install",
    tag: "v0.6.1",
    repo: "EdelmarSchneider/permark",
    pubDate: "2026-10-20T19:02:17.540Z",
    assets,
    signature: (name) => `sig of ${name}`,
    ...overrides,
  });
}

describe("updaterJson", () => {
  it("writes the 0.6.1 shape: same keys, order, urls and signatures", () => {
    const json = build(ASSETS);
    expect(Object.keys(json)).toEqual(["version", "notes", "pub_date", "platforms"]);
    expect(json.version).toBe("0.6.1");
    expect(json.notes).toBe("**PerMark** notes\n\n### Install");
    expect(json.pub_date).toBe("2026-10-20T19:02:17.540Z");
    const msi = {
      signature: "sig of PerMark_0.6.1_x64_en-US.msi.sig",
      url: `${BASE}PerMark_0.6.1_x64_en-US.msi`,
    };
    const app = {
      signature: "sig of PerMark_universal.app.tar.gz.sig",
      url: `${BASE}PerMark_universal.app.tar.gz`,
    };
    const appImage = {
      signature: "sig of PerMark_0.6.1_amd64.AppImage.sig",
      url: `${BASE}PerMark_0.6.1_amd64.AppImage`,
    };
    expect(json.platforms).toEqual({
      "windows-x86_64": msi,
      "windows-x86_64-msi": msi,
      "windows-x86_64-nsis": {
        signature: "sig of PerMark_0.6.1_x64-setup.exe.sig",
        url: `${BASE}PerMark_0.6.1_x64-setup.exe`,
      },
      "darwin-aarch64": app,
      "darwin-x86_64": app,
      "darwin-aarch64-app": app,
      "darwin-x86_64-app": app,
      "linux-x86_64": appImage,
      "linux-x86_64-appimage": appImage,
      "linux-x86_64-deb": {
        signature: "sig of PerMark_0.6.1_amd64.deb.sig",
        url: `${BASE}PerMark_0.6.1_amd64.deb`,
      },
      "linux-x86_64-rpm": {
        signature: "sig of PerMark-0.6.1-1.x86_64.rpm.sig",
        url: `${BASE}PerMark-0.6.1-1.x86_64.rpm`,
      },
    });
    expect(Object.keys(json.platforms)).toEqual([
      "windows-x86_64",
      "windows-x86_64-msi",
      "windows-x86_64-nsis",
      "darwin-aarch64",
      "darwin-x86_64",
      "darwin-aarch64-app",
      "darwin-x86_64-app",
      "linux-x86_64",
      "linux-x86_64-appimage",
      "linux-x86_64-deb",
      "linux-x86_64-rpm",
    ]);
  });

  it("keeps the signature text byte for byte (no trim, CRLF kept)", () => {
    const json = build(ASSETS, { signature: () => "dW50cnVzdGVk\r\nIGNvbW1lbnQ=\n" });
    expect(json.platforms["linux-x86_64-deb"]?.signature).toBe("dW50cnVzdGVk\r\nIGNvbW1lbnQ=\n");
  });

  it("does not depend on the order of the assets", () => {
    expect(build([...ASSETS].reverse())).toEqual(build(ASSETS));
  });

  it("encodes the tag and asset names in urls", () => {
    const assets = ASSETS.map((name) => name.replaceAll("0.6.1", "0.7.0-rc+1"));
    const json = build(assets, { version: "0.7.0-rc+1", tag: "v0.7.0-rc+1" });
    expect(json.platforms["windows-x86_64-nsis"]?.url).toBe(
      "https://github.com/EdelmarSchneider/permark/releases/download/v0.7.0-rc%2B1/PerMark_0.7.0-rc%2B1_x64-setup.exe",
    );
  });

  it.each([
    ["PerMark_0.6.1_x64_en-US.msi.sig", /Windows MSI: 0 signature/],
    ["PerMark_0.6.1_x64-setup.exe.sig", /Windows NSIS: 0 signature/],
    ["PerMark_universal.app.tar.gz.sig", /macOS universal app: 0 signature/],
    ["PerMark_0.6.1_amd64.AppImage.sig", /Linux AppImage: 0 signature/],
    ["PerMark_0.6.1_amd64.deb.sig", /Linux deb: 0 signature/],
    ["PerMark-0.6.1-1.x86_64.rpm.sig", /Linux rpm: 0 signature/],
  ])("refuses a release without %s (a platform job failed or was skipped)", (missing, error) => {
    expect(() => build(ASSETS.filter((name) => name !== missing))).toThrow(error);
  });

  it("refuses a signature without its installer", () => {
    expect(() => build(ASSETS.filter((name) => name !== "PerMark_0.6.1_amd64.deb"))).toThrow(
      /has no asset PerMark_0.6.1_amd64.deb/,
    );
  });

  it("refuses two bundles for one rule (e.g. a second MSI language)", () => {
    const assets = [...ASSETS, "PerMark_0.6.1_x64_pt-BR.msi", "PerMark_0.6.1_x64_pt-BR.msi.sig"];
    expect(() => build(assets)).toThrow(/Windows MSI: 2 signature/);
  });

  it.each([
    "PerMark_0.6.1_aarch64.dmg.sig",
    "PerMark_0.6.1_arm64-setup.exe.sig",
    "PerMark_0.6.1_arm64.deb.sig",
    "PerMark_aarch64.app.tar.gz.sig",
  ])("refuses an updater signature no rule knows: %s", (unknown) => {
    expect(() => build([...ASSETS, unknown])).toThrow(/matches 0 updater rules/);
  });

  it("refuses an empty signature", () => {
    expect(() =>
      build(ASSETS, { signature: (name) => (name.endsWith(".deb.sig") ? " \n" : "sig") }),
    ).toThrow(/Linux deb: .* is empty/);
  });

  it("refuses a tag that is not the app's version", () => {
    expect(() => build(ASSETS, { tag: "v0.6.2" })).toThrow(/not the app's version v0.6.1/);
  });

  it("refuses a repo that is not owner/name", () => {
    expect(() => build(ASSETS, { repo: "EdelmarSchneider/permark/../x y" })).toThrow(
      /not owner\/name/,
    );
  });
});

describe("parseRelease", () => {
  it("reads tag, body and asset names", () => {
    const text = JSON.stringify({
      tag_name: "v0.6.1",
      body: "notes\r\nline",
      draft: true,
      assets: [{ id: 1, name: "a.deb" }, { name: "a.deb.sig" }],
    });
    expect(parseRelease(text)).toEqual({
      tag: "v0.6.1",
      notes: "notes\r\nline",
      assets: ["a.deb", "a.deb.sig"],
    });
  });

  it("treats a null body as empty notes", () => {
    expect(parseRelease(JSON.stringify({ tag_name: "v1.0.0", body: null, assets: [] })).notes).toBe(
      "",
    );
  });

  it.each([
    ["not an object", "null", /not an object/],
    ["no tag", JSON.stringify({ assets: [] }), /no tag_name/],
    ["no assets", JSON.stringify({ tag_name: "v1" }), /no assets/],
    [
      "path in a name",
      JSON.stringify({ tag_name: "v1", assets: [{ name: "../x.sig" }] }),
      /bad asset name/,
    ],
    [
      "backslash in a name",
      JSON.stringify({ tag_name: "v1", assets: [{ name: "a\\b.sig" }] }),
      /bad asset name/,
    ],
    ["empty name", JSON.stringify({ tag_name: "v1", assets: [{ name: "" }] }), /bad asset name/],
  ])("refuses %s", (_, text, error) => {
    expect(() => parseRelease(text)).toThrow(error);
  });
});
