import { afterEach, describe, expect, it, vi } from "vitest";
import { mockAssetUrl } from "./mockDocs";

describe("mockAssetUrl", () => {
  afterEach(() => {
    vi.unstubAllEnvs();
  });

  it("draws a labelled placeholder with no server (dev:mock, tests)", () => {
    const url = mockAssetUrl("docs/img/logo.png");
    expect(url.startsWith("data:image/svg+xml")).toBe(true);
    expect(decodeURIComponent(url)).toContain("logo.png");
  });

  it("points at the corpus file the deep suite's server serves, path parts encoded", () => {
    vi.stubEnv("MODE", "deep");
    expect(mockAssetUrl("demo/assets/crew.svg")).toBe(
      "/@permark-deep/project/demo/assets/crew.svg",
    );
    expect(mockAssetUrl("corpus/a b/#1?.wav")).toBe(
      "/@permark-deep/project/corpus/a%20b/%231%3F.wav",
    );
  });
});
