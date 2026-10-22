import { describe, expect, it } from "vitest";
import { isMockModule, mockModulesIn } from "./no-mock-in-production";

describe("isMockModule", () => {
  it("matches the mock backend modules and fixtures, on any separator", () => {
    expect(isMockModule("/repo/app/src/api/mock.ts")).toBe(true);
    expect(isMockModule("/repo/app/src/api/mockProjects.ts")).toBe(true);
    expect(isMockModule("/repo/app/src/api/generated/mock-documents.json")).toBe(true);
    expect(isMockModule("F:\\My Docs\\app\\src\\api\\mockEdit.ts")).toBe(true);
    expect(isMockModule("/repo/app/src/api/generated/mock-edit-sources.json?import")).toBe(true);
  });

  it("leaves the real backend, generated types and look-alike names alone", () => {
    expect(isMockModule("/repo/app/src/api/tauri.ts")).toBe(false);
    expect(isMockModule("/repo/app/src/api/index.ts")).toBe(false);
    expect(isMockModule("/repo/app/src/api/generated/AppSettings.ts")).toBe(false);
    expect(isMockModule("/repo/app/src/core/mockup/view.ts")).toBe(false);
    expect(isMockModule("/repo/app/src/api/mock/readme.md")).toBe(false);
    expect(isMockModule("/repo/node_modules/some-lib/src/api/mock.ts.map")).toBe(false);
  });
});

describe("mockModulesIn", () => {
  it("lists every mock module per chunk, skips assets", () => {
    const found = mockModulesIn({
      "assets/index.js": {
        type: "chunk",
        fileName: "assets/index.js",
        moduleIds: ["/a/src/main.ts", "/a/src/api/tauri.ts"],
      },
      "assets/api.js": {
        type: "chunk",
        fileName: "assets/api.js",
        moduleIds: ["/a/src/api/mock.ts", "/a/src/api/generated/mock-documents.json"],
      },
      "assets/app.css": { type: "asset" },
    });
    expect(found).toEqual([
      "assets/api.js: /a/src/api/mock.ts",
      "assets/api.js: /a/src/api/generated/mock-documents.json",
    ]);
  });

  it("is empty for a clean bundle", () => {
    expect(
      mockModulesIn({
        "x.js": { type: "chunk", fileName: "x.js", moduleIds: ["/a/src/api/tauri.ts"] },
      }),
    ).toEqual([]);
  });
});
