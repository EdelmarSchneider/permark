import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The backend is replaced: these tests are about what is kept, when, and what the field says.
const fake = vi.hoisted(() => ({
  editorCommand: vi.fn(async () => ""),
  setEditorCommand: vi.fn(async (_command: string) => {}),
}));
vi.mock("../api", () => ({ api: fake }));

import { editorCommand } from "./editorCommand.svelte";

beforeEach(() => {
  vi.useFakeTimers();
  fake.editorCommand.mockReset().mockResolvedValue("");
  fake.setEditorCommand.mockReset().mockResolvedValue(undefined);
  editorCommand.command = "";
  editorCommand.draft = "";
  editorCommand.customPicked = false;
  editorCommand.error = null;
});

afterEach(async () => {
  await editorCommand.flush();
  vi.useRealTimers();
});

describe("loading", () => {
  it("shows the saved command and the preset it is", async () => {
    fake.editorCommand.mockResolvedValue("code -g {path}:{line}:{col}");
    await editorCommand.load();
    expect(editorCommand.command).toBe("code -g {path}:{line}:{col}");
    expect(editorCommand.draft).toBe("code -g {path}:{line}:{col}");
    expect(editorCommand.preset).toBe("vscode");
  });

  it("is the system's default app when nothing is saved", async () => {
    await editorCommand.load();
    expect(editorCommand.preset).toBe("system");
  });

  it("keeps what it had when the backend does not answer", async () => {
    editorCommand.command = "zed {path}";
    fake.editorCommand.mockRejectedValue(new Error("down"));
    await editorCommand.load();
    expect(editorCommand.command).toBe("zed {path}");
  });

  it("does not take the text out of the field while it is being typed", async () => {
    editorCommand.type("vim +{line} {path}");
    fake.editorCommand.mockResolvedValue("code {path}");
    await editorCommand.load();
    expect(editorCommand.draft).toBe("vim +{line} {path}");
    expect(editorCommand.command).toBe("code {path}");
  });
});

describe("keeping the command", () => {
  it("waits for a pause in the typing, then keeps the last text once", async () => {
    editorCommand.type("c");
    editorCommand.type("co");
    editorCommand.type("code {path}");
    expect(fake.setEditorCommand).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1000);
    expect(fake.setEditorCommand).toHaveBeenCalledTimes(1);
    expect(fake.setEditorCommand).toHaveBeenCalledWith("code {path}");
    expect(editorCommand.command).toBe("code {path}");
    expect(editorCommand.error).toBeNull();
  });

  it("keeps at once when the field closes", async () => {
    editorCommand.type("zed {path}:{line}");
    await editorCommand.flush();
    expect(fake.setEditorCommand).toHaveBeenCalledWith("zed {path}:{line}");
    // nothing is left to keep
    await editorCommand.flush();
    expect(fake.setEditorCommand).toHaveBeenCalledTimes(1);
  });

  it("keeps the texts in the order they were typed", async () => {
    const order: string[] = [];
    fake.setEditorCommand.mockImplementation(async (command: string) => {
      if (command === "first") await Promise.resolve();
      order.push(command);
    });
    editorCommand.type("first");
    await editorCommand.flush();
    editorCommand.type("second");
    await editorCommand.flush();
    expect(order).toEqual(["first", "second"]);
  });

  it("trims what is in force", async () => {
    editorCommand.type("  code {path}  ");
    await editorCommand.flush();
    expect(editorCommand.command).toBe("code {path}");
  });

  it("says why a command was not kept, and keeps the last good one in force", async () => {
    editorCommand.type("code {path}");
    await editorCommand.flush();
    fake.setEditorCommand.mockRejectedValue({
      kind: "editor",
      reason: "template",
      detail: "unknown placeholder {file}",
      message: "raw",
    });
    editorCommand.type("code {file}");
    await editorCommand.flush();
    expect(editorCommand.error).toBe("Not saved: unknown placeholder {file}");
    expect(editorCommand.command).toBe("code {path}");
    expect(editorCommand.draft).toBe("code {file}");
    // typing it right clears the message
    fake.setEditorCommand.mockResolvedValue(undefined);
    editorCommand.type("code {path}:{line}");
    await editorCommand.flush();
    expect(editorCommand.error).toBeNull();
    expect(editorCommand.command).toBe("code {path}:{line}");
  });

  it("falls back to the message of an error with no detail", async () => {
    fake.setEditorCommand.mockRejectedValue(new Error("disk full"));
    editorCommand.type("x");
    await editorCommand.flush();
    expect(editorCommand.error).toBe("Not saved: disk full");
  });
});

describe("the list of editors", () => {
  it("fills a preset's command in and keeps it", async () => {
    editorCommand.pick("cursor");
    expect(editorCommand.draft).toBe("cursor -g {path}:{line}:{col}");
    expect(editorCommand.preset).toBe("cursor");
    await editorCommand.flush();
    expect(fake.setEditorCommand).toHaveBeenCalledWith("cursor -g {path}:{line}:{col}");
  });

  it("clears the command for the system default", async () => {
    editorCommand.pick("zed");
    await editorCommand.flush();
    editorCommand.pick("system");
    await editorCommand.flush();
    expect(fake.setEditorCommand).toHaveBeenLastCalledWith("");
    expect(editorCommand.command).toBe("");
  });

  it("keeps the text when Custom is picked, and shows Custom while it is edited", async () => {
    editorCommand.pick("vscode");
    editorCommand.pick("custom");
    expect(editorCommand.draft).toBe("code -g {path}:{line}:{col}");
    expect(editorCommand.preset).toBe("custom");
    expect(fake.setEditorCommand).not.toHaveBeenCalled();
  });

  it("shows Custom for text that is no preset, and the preset again when the text becomes one", () => {
    editorCommand.type("vim +{line} {path}");
    expect(editorCommand.preset).toBe("custom");
    editorCommand.type("zed {path}:{line}:{col}");
    expect(editorCommand.preset).toBe("zed");
  });
});
