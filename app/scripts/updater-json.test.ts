import { readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import { parseRelease, publicKeyOf, updaterJson, verifySignature } from "./updater-json.mjs";

// Asset names of the 0.6.1 release (unchanged since 0.6.0; `distribution.md` § Release pipeline), minus the `.msi`
// (no longer built), and with the macOS updater bundle named as tauri-action 1.x names it: with the version, as in
// every other installer.
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
  "PerMark_0.6.1_universal.app.tar.gz",
  "PerMark_0.6.1_universal.app.tar.gz.sig",
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
    verify: () => {},
    ...overrides,
  });
}

describe("updaterJson", () => {
  it("writes the 0.6.1 shape without the .msi: same keys, order, urls and signatures", () => {
    const json = build(ASSETS);
    expect(Object.keys(json)).toEqual(["version", "notes", "pub_date", "platforms"]);
    expect(json.version).toBe("0.6.1");
    expect(json.notes).toBe("**PerMark** notes\n\n### Install");
    expect(json.pub_date).toBe("2026-10-20T19:02:17.540Z");
    // copies installed from an `.msi` look up `windows-x86_64-msi`, then `windows-x86_64`: both reach the `-setup.exe`
    const nsis = {
      signature: "sig of PerMark_0.6.1_x64-setup.exe.sig",
      url: `${BASE}PerMark_0.6.1_x64-setup.exe`,
    };
    const app = {
      signature: "sig of PerMark_0.6.1_universal.app.tar.gz.sig",
      url: `${BASE}PerMark_0.6.1_universal.app.tar.gz`,
    };
    const appImage = {
      signature: "sig of PerMark_0.6.1_amd64.AppImage.sig",
      url: `${BASE}PerMark_0.6.1_amd64.AppImage`,
    };
    expect(json.platforms).toEqual({
      "windows-x86_64": nsis,
      "windows-x86_64-nsis": nsis,
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
    ["PerMark_0.6.1_x64-setup.exe.sig", /Windows NSIS: 0 signature/],
    ["PerMark_0.6.1_universal.app.tar.gz.sig", /macOS universal app: 0 signature/],
    ["PerMark_0.6.1_amd64.AppImage.sig", /Linux AppImage: 0 signature/],
    ["PerMark_0.6.1_amd64.deb.sig", /Linux deb: 0 signature/],
    ["PerMark-0.6.1-1.x86_64.rpm.sig", /Linux rpm: 0 signature/],
  ])("refuses a release without %s (a platform job failed or was skipped)", (missing, error) => {
    expect(() => build(ASSETS.filter((name) => name !== missing))).toThrow(error);
  });

  it("still reads a macOS updater bundle without the version (releases built before tauri-action 1.x)", () => {
    const assets = ASSETS.map((name) =>
      name.replace("PerMark_0.6.1_universal.app", "PerMark_universal.app"),
    );
    expect(build(assets).platforms["darwin-aarch64"]).toEqual({
      signature: "sig of PerMark_universal.app.tar.gz.sig",
      url: `${BASE}PerMark_universal.app.tar.gz`,
    });
  });

  it("refuses a signature without its installer", () => {
    expect(() => build(ASSETS.filter((name) => name !== "PerMark_0.6.1_amd64.deb"))).toThrow(
      /has no asset PerMark_0.6.1_amd64.deb/,
    );
  });

  it("refuses two bundles for one rule (e.g. two Windows installers)", () => {
    const assets = [
      ...ASSETS,
      "PerMark_0.6.1-rc_x64-setup.exe",
      "PerMark_0.6.1-rc_x64-setup.exe.sig",
    ];
    expect(() => build(assets)).toThrow(/Windows NSIS: 2 signature/);
  });

  it.each([
    // the `.msi` is no longer built: one in a release is a stray bundle, not an update
    "PerMark_0.6.1_x64_en-US.msi.sig",
    "PerMark_0.6.1_aarch64.dmg.sig",
    "PerMark_0.6.1_arm64-setup.exe.sig",
    "PerMark_0.6.1_arm64.deb.sig",
    "PerMark_0.6.1_aarch64.app.tar.gz.sig",
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

  it("checks every installer against its signature, once each", () => {
    const checked: string[] = [];
    build(ASSETS, { verify: (bundle, signature) => checked.push(`${bundle} <- ${signature}`) });
    expect(checked).toEqual([
      "PerMark_0.6.1_x64-setup.exe <- sig of PerMark_0.6.1_x64-setup.exe.sig",
      "PerMark_0.6.1_universal.app.tar.gz <- sig of PerMark_0.6.1_universal.app.tar.gz.sig",
      "PerMark_0.6.1_amd64.AppImage <- sig of PerMark_0.6.1_amd64.AppImage.sig",
      "PerMark_0.6.1_amd64.deb <- sig of PerMark_0.6.1_amd64.deb.sig",
      "PerMark-0.6.1-1.x86_64.rpm <- sig of PerMark-0.6.1-1.x86_64.rpm.sig",
    ]);
  });

  it("refuses a release with one installer its signature does not match", () => {
    const verify = (bundle: string) => {
      if (bundle.endsWith(".rpm")) throw new Error("the signature does not match the file");
    };
    expect(() => build(ASSETS, { verify })).toThrow(
      /Linux rpm: PerMark-0.6.1-1.x86_64.rpm.sig: the signature does not match the file/,
    );
  });
});

// A real updater signature: `tauri signer generate` made a throwaway key pair (its private key was thrown away), then
// `tauri signer sign` signed FIXTURE_FILE with it. The texts are what the CLI writes (`.sig` file, `.key.pub` file).
const FIXTURE_FILE = "PerMark updater signature fixture\n";
const FIXTURE_PUBKEY =
  "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDhFMjA3ODNGMzhEMTBCRjkKUldUNUM5RTRQM2dnampPVmMrWGxEZ291b3RRckRJbFBIYUR5UldjUmxuOXA3WFJPUGVNRkg2aDQK";
const FIXTURE_SIG =
  "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVUNUM5RTRQM2dnanUxRldidi9uTXdDa2tNNFhFdElyQzFKdDVBNk1RQ3UxVWd1U1o0RWxzdVJsRjdPdGw5c2hoVXpJaHJEOUtLOEZQVFN1RHpMU0p6N054aTN2c3JCdGdjPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzk1MjQ2NjY3CWZpbGU6YnVuZGxlLnR4dApraDVEQnk5ODFhVzJQd1dpYkQxR1BQMlN5U0FQNkhNakdpcWdqNk8xUUxMR0tzRHBHeEFJNUhDRHoyb0MvMldBN0VTc0hyYTU2Ym4yTGZXU2hicCtCQT09Cg==";

const APP_PUBKEY: string = JSON.parse(
  readFileSync(join(import.meta.dirname, "..", "src-tauri", "tauri.conf.json"), "utf8"),
).plugins.updater.pubkey;

/** The signature text with one line changed, encoded again as the `.sig` file holds it. */
function editedSig(edit: (text: string) => string): string {
  return Buffer.from(edit(Buffer.from(FIXTURE_SIG, "base64").toString("utf8"))).toString("base64");
}

describe("verifySignature", () => {
  const file = Buffer.from(FIXTURE_FILE);

  it("accepts a file signed by the key, as `tauri signer sign` signs it", () => {
    expect(() => verifySignature(FIXTURE_SIG, file, FIXTURE_PUBKEY)).not.toThrow();
    // the `.sig` file text as downloaded, with a line end after it
    expect(() => verifySignature(`${FIXTURE_SIG}\n`, file, FIXTURE_PUBKEY)).not.toThrow();
  });

  it("refuses a file with one byte changed", () => {
    const tampered = Buffer.from(file);
    tampered[0] = (tampered[0] ?? 0) ^ 1;
    expect(() => verifySignature(FIXTURE_SIG, tampered, FIXTURE_PUBKEY)).toThrow(
      /does not match the file/,
    );
  });

  it("refuses a signature whose trusted comment was changed", () => {
    const sig = editedSig((text) => text.replace("timestamp:1795246667", "timestamp:1795246668"));
    expect(() => verifySignature(sig, file, FIXTURE_PUBKEY)).toThrow(/trusted comment/);
  });

  it("refuses a signature made by another key than the app trusts", () => {
    expect(() => verifySignature(FIXTURE_SIG, file, APP_PUBKEY)).toThrow(
      /key 8E20783F38D10BF9.*trusts key E5B5D80713C61885/,
    );
  });

  it("refuses text that is not a signature", () => {
    expect(() => verifySignature("bm90IGEgc2lnbmF0dXJl", file, FIXTURE_PUBKEY)).toThrow(
      /not a minisign signature/,
    );
    const legacy = editedSig((text) => text.replace("\nRUT5", "\nRWT5"));
    expect(() => verifySignature(legacy, file, FIXTURE_PUBKEY)).toThrow(/algorithm/);
  });
});

describe("publicKeyOf", () => {
  it("reads the key id of the app's updater key", () => {
    expect(publicKeyOf(APP_PUBKEY).id).toBe("E5B5D80713C61885");
    expect(publicKeyOf(FIXTURE_PUBKEY).id).toBe("8E20783F38D10BF9");
  });

  it("refuses text that is not a public key", () => {
    expect(() => publicKeyOf("bm90IGEga2V5")).toThrow(/not a minisign public key/);
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
