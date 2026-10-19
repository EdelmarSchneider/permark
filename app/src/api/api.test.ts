import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
const show = vi.fn();
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ show }) }));

const { call, tauriApi } = await import("./tauri");
const { MOCK_VERSION, mockApi } = await import("./mock");
const { errorMessage } = await import("./error");

beforeEach(() => {
  invoke.mockReset();
  show.mockReset();
});

describe("call (typed invoke)", () => {
  it("passes the command name + args and returns the value", async () => {
    invoke.mockResolvedValue({ version: "1.2.3" });
    expect(await call("app_info", {})).toEqual({ ok: true, value: { version: "1.2.3" } });
    expect(invoke).toHaveBeenCalledWith("app_info", {});
  });

  it("turns a rejection into a typed error, never a throw", async () => {
    invoke.mockRejectedValue("command app_info not found");
    expect(await tauriApi.appInfo()).toEqual({
      ok: false,
      error: { kind: "command", command: "app_info", message: "command app_info not found" },
    });
  });
});

describe("showWindow", () => {
  it("shows the window", async () => {
    show.mockResolvedValue(undefined);
    expect(await tauriApi.showWindow()).toEqual({ ok: true, value: undefined });
    expect(show).toHaveBeenCalledOnce();
  });

  it("reports a refused permission as a window error", async () => {
    show.mockRejectedValue(new Error("window.show not allowed"));
    expect(await tauriApi.showWindow()).toEqual({
      ok: false,
      error: { kind: "window", message: "window.show not allowed" },
    });
  });
});

describe("mock backend", () => {
  it("answers every method of the interface without Tauri", async () => {
    expect(await mockApi.appInfo()).toEqual({ ok: true, value: { version: MOCK_VERSION } });
    expect(await mockApi.showWindow()).toEqual({ ok: true, value: undefined });
    expect(invoke).not.toHaveBeenCalled();
  });
});

describe("errorMessage (adversarial)", () => {
  it("reads strings, errors and plain objects", () => {
    expect(errorMessage("boom")).toBe("boom");
    expect(errorMessage(new TypeError("bad"))).toBe("bad");
    expect(errorMessage({ code: 5 })).toBe('{"code":5}');
  });

  it("never throws on cyclic, undefined, symbol or bigint values", () => {
    const cyclic: Record<string, unknown> = {};
    cyclic["self"] = cyclic;
    expect(errorMessage(cyclic)).toBe("unknown error");
    expect(errorMessage(undefined)).toBe("undefined");
    expect(errorMessage(Symbol("s"))).toBe("Symbol(s)");
    expect(errorMessage(10n)).toBe("unknown error");
  });
});
