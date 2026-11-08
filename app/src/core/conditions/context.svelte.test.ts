import { describe, expect, it } from "vitest";
import { hostPlatform, neutralContext } from "./context.svelte";

describe("the platform before the backend's context arrives", () => {
  it("is the system the web view runs on", () => {
    expect(hostPlatform({ userAgentData: { platform: "macOS" }, platform: "Win32" })).toBe("mac");
    expect(hostPlatform({ userAgentData: { platform: "Windows" } })).toBe("windows");
    expect(hostPlatform({ userAgentData: { platform: "" }, platform: "MacIntel" })).toBe("mac");
    expect(hostPlatform({ platform: "Linux x86_64" })).toBe("linux");
    expect(hostPlatform({ platform: "Win32" })).toBe("windows");
  });

  it("is Windows when the web view does not say", () => {
    expect(hostPlatform({ platform: "" })).toBe("windows");
    expect(hostPlatform({})).toBe("windows");
  });

  it("is what the neutral context holds", () => {
    expect(neutralContext().platform).toBe(hostPlatform());
  });
});
