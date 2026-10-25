import { describe, expect, it, vi } from "vitest";

import { mockPictureRefusal } from "./mockRemote";

vi.mock("@tauri-apps/api/core", () => ({ invoke: vi.fn(), convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({ getCurrentWebview: vi.fn() }));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

// The same cases as `permark_core::remote` tests: the mock refuses what the backend refuses, with the same error.
describe("the mock's web picture address rules", () => {
  it("refuses this computer and the local network, with the host as written", () => {
    const cases: [string, string][] = [
      ["https://localhost/a.png", "localhost"],
      ["https://LOCALHOST:8443/a.png", "LOCALHOST"],
      ["https://localhost./a.png", "localhost."],
      ["https://router.localhost/a.png", "router.localhost"],
      ["https://127.0.0.1/a.png", "127.0.0.1"],
      ["https://127.0.0.1.:443/a.png", "127.0.0.1."],
      ["https://10.1.2.3/a.png", "10.1.2.3"],
      ["https://172.20.0.5/a.png", "172.20.0.5"],
      ["https://192.168.1.1/a.png", "192.168.1.1"],
      ["https://169.254.169.254/latest/meta-data/", "169.254.169.254"],
      ["https://100.100.100.200/a.png", "100.100.100.200"],
      ["https://0.0.0.0/a.png", "0.0.0.0"],
      ["https://198.18.0.1/a.png", "198.18.0.1"],
      ["https://240.0.0.1/a.png", "240.0.0.1"],
      ["https://[::1]/a.png", "[::1]"],
      ["https://[::1]:8443/a.png", "[::1]"],
      ["https://[::]/a.png", "[::]"],
      ["https://[fd00::1]/a.png", "[fd00::1]"],
      ["https://[fe80::1]/a.png", "[fe80::1]"],
      ["https://[fec0::1]/a.png", "[fec0::1]"],
      ["https://[ff02::1]/a.png", "[ff02::1]"],
      ["https://[2001:db8::1]/a.png", "[2001:db8::1]"],
      ["https://[::ffff:127.0.0.1]/a.png", "[::ffff:127.0.0.1]"],
      ["https://[::ffff:a9fe:a9fe]/a.png", "[::ffff:a9fe:a9fe]"],
      ["https://[::ffff:0:192.168.1.1]/a.png", "[::ffff:0:192.168.1.1]"],
      ["https://[::10.0.0.1]/a.png", "[::10.0.0.1]"],
      ["https://[64:ff9b::a9fe:a9fe]/a.png", "[64:ff9b::a9fe:a9fe]"],
      ["https://[2002:c0a8:101::1]/a.png", "[2002:c0a8:101::1]"],
      ["https://192.168.1.1?x", "192.168.1.1"],
    ];
    for (const [url, host] of cases) {
      expect(mockPictureRefusal(url), url).toEqual({
        kind: "denied",
        subject: host,
        detail: "address",
      });
    }
  });

  it("refuses a number that is not a plain dotted IPv4 address", () => {
    for (const url of [
      "https://127.1/a.png",
      "https://2130706433/a.png",
      "https://0x7f000001/a.png",
      "https://0x7f.0.0.1/a.png",
      "https://0177.0.0.1/a.png",
      "https://127.000.000.001/a.png",
      "https://1.2.3.4.5/a.png",
      "https://8.8.8.256/a.png",
      "https://example.0x10/a.png",
      "https://[fe80::1%25eth0]/a.png",
      "https://[127.0.0.1]/a.png",
      "https://[::1/a.png",
    ]) {
      expect(mockPictureRefusal(url), url).toEqual({
        kind: "invalidData",
        subject: null,
        detail: "address",
      });
    }
  });

  it("lets public hosts through", () => {
    for (const url of [
      "https://example.com/a.png",
      "https://93.184.215.14/a.png",
      "https://100.63.255.255/a.png",
      "https://172.32.0.1/a.png",
      "https://[2606:4700:4700::1111]/a.png",
      "https://[::ffff:8.8.8.8]/a.png",
      "https://[64:ff9b::808:808]/a.png",
      "https://[2002:808:808::1]/a.png",
      "https://localhost.example.com/a.png",
      "https://my-localhost/a.png",
      "https://127.0.0.1.example.com/a.png",
      "https://10.example/a.png",
    ]) {
      expect(mockPictureRefusal(url), url).toBeNull();
    }
  });

  it("keeps the scheme and shape refusals", () => {
    expect(mockPictureRefusal("http://example.com/a.png")).toEqual({
      kind: "unsupported",
      subject: null,
      detail: "https",
    });
    for (const url of [
      "https://",
      "https://user:pw@example.com/a.png",
      "https://:443/a.png",
      "https://e.com/a b",
    ]) {
      expect(mockPictureRefusal(url), url).toEqual({
        kind: "invalidData",
        subject: null,
        detail: "address",
      });
    }
  });

  it("answers through the mock backend like the app", async () => {
    const { createMockApi } = await import("./mock");
    const api = createMockApi();
    const result = await api.remotePicture("https://169.254.169.254/latest/meta-data/");
    expect(result).toEqual({
      ok: false,
      error: {
        kind: "backend",
        command: "remote_picture",
        error: { kind: "denied", subject: "169.254.169.254", detail: "address" },
      },
    });
    expect((await api.remotePicture("https://example.com/a.png")).ok).toBe(true);
  });
});
