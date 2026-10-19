// @vitest-environment jsdom
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { DragDropEvent } from "@tauri-apps/api/webview";

const invoke = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke, convertFileSrc: vi.fn() }));
vi.mock("@tauri-apps/api/window", () => ({ getCurrentWindow: () => ({ show: vi.fn() }) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn() }));
let dragHandler: ((event: { payload: DragDropEvent }) => void) | null = null;
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: (handler: (event: { payload: DragDropEvent }) => void) => {
      dragHandler = handler;
      return Promise.resolve(() => {});
    },
  }),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));

const { cssPoint, mapDrop, tauriApi } = await import("./tauri");

const physical = (x: number, y: number): { type: "Physical"; x: number; y: number } =>
  ({ type: "Physical", x, y }) as never;

beforeEach(() => {
  invoke.mockReset();
  dragHandler = null;
});

describe("the window point of a drop", () => {
  it("is the physical position over the scale of the screen", () => {
    expect(cssPoint({ x: 300, y: 150 }, 1.5)).toEqual({ x: 200, y: 100 });
    expect(cssPoint({ x: 300, y: 150 }, 1)).toEqual({ x: 300, y: 150 });
    expect(cssPoint({ x: 0, y: 0 }, 2)).toEqual({ x: 0, y: 0 });
  });

  it("falls back to no scaling when the scale is nonsense", () => {
    for (const bad of [0, -1, Number.NaN, Number.POSITIVE_INFINITY * 0]) {
      expect(cssPoint({ x: 10, y: 20 }, bad)).toEqual({ x: 10, y: 20 });
    }
  });
});

describe("mapDrop", () => {
  it("enter carries the paths and the point, and the paths are kept for the over events", () => {
    const entered = mapDrop(
      { type: "enter", paths: ["C:/a", "C:/b"], position: physical(200, 100) } as DragDropEvent,
      [],
      2,
    );
    expect(entered.event).toEqual({
      type: "over",
      count: 2,
      paths: ["C:/a", "C:/b"],
      x: 100,
      y: 50,
    });
    const over = mapDrop(
      { type: "over", position: physical(400, 200) } as DragDropEvent,
      entered.held,
      2,
    );
    expect(over.event).toEqual({ type: "over", count: 2, paths: ["C:/a", "C:/b"], x: 200, y: 100 });
  });

  it("drop carries the paths and the point and forgets what was held", () => {
    const dropped = mapDrop(
      { type: "drop", paths: ["C:/文档 x.md"], position: physical(10, 20) } as DragDropEvent,
      ["C:/old"],
      1,
    );
    expect(dropped.event).toEqual({ type: "drop", paths: ["C:/文档 x.md"], x: 10, y: 20 });
    expect(dropped.held).toEqual([]);
  });

  it("leave clears what was held", () => {
    const left = mapDrop({ type: "leave" } as DragDropEvent, ["C:/a"], 1);
    expect(left).toEqual({ event: { type: "leave" }, held: [] });
  });

  it("onDrop hands the app's events to its handler, one drag after another", async () => {
    const seen: unknown[] = [];
    await tauriApi.onDrop((event) => seen.push(event));
    const send = (payload: object): void => dragHandler?.({ payload: payload as DragDropEvent });
    send({ type: "enter", paths: ["C:/a"], position: physical(1, 2) });
    send({ type: "over", position: physical(3, 4) });
    send({ type: "leave" });
    send({ type: "over", position: physical(5, 6) });
    expect(seen).toEqual([
      { type: "over", count: 1, paths: ["C:/a"], x: expect.any(Number), y: expect.any(Number) },
      { type: "over", count: 1, paths: ["C:/a"], x: expect.any(Number), y: expect.any(Number) },
      { type: "leave" },
      { type: "over", count: 0, paths: [], x: expect.any(Number), y: expect.any(Number) },
    ]);
  });
});

describe("the import commands", () => {
  it("pass their arguments by name", async () => {
    invoke.mockResolvedValue({ planId: 1 });
    await tauriApi.planImport("docs", ["C:/a"]);
    expect(invoke).toHaveBeenLastCalledWith("fileops_import_plan", {
      target: "docs",
      sources: ["C:/a"],
    });
    await tauriApi.applyImport(1, "move", "skip");
    expect(invoke).toHaveBeenLastCalledWith("fileops_import_apply", {
      planId: 1,
      mode: "move",
      conflicts: "skip",
    });
    await tauriApi.cancelImport(1);
    expect(invoke).toHaveBeenLastCalledWith("fileops_import_cancel", { planId: 1 });
  });

  it("read a picture as bytes, and a refusal keeps its shape", async () => {
    invoke.mockResolvedValueOnce(new Uint8Array([1, 2, 3]).buffer);
    const read = await tauriApi.readPicture("C:/p.png");
    expect(read.ok && Array.from(read.value)).toEqual([1, 2, 3]);
    expect(invoke).toHaveBeenLastCalledWith("picture_read", { path: "C:/p.png" });
    invoke.mockRejectedValueOnce({ kind: "unsupported", subject: "C:/p.png", detail: "content" });
    const refused = await tauriApi.readPicture("C:/p.png");
    expect(refused).toEqual({
      ok: false,
      error: {
        kind: "backend",
        command: "picture_read",
        error: { kind: "unsupported", subject: "C:/p.png", detail: "content" },
      },
    });
  });
});
