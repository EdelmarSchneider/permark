// @vitest-environment jsdom
// The file manager card: each switch shows what the registry holds, also after a change the backend refused, and a
// card whose first read failed says why instead of staying blank.
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { ApiError, Result, ShellState } from "../../api";

type Answer = Result<ShellState>;

const backend = vi.hoisted(() => ({
  state: [] as (() => Answer)[],
  set: [] as (() => Answer)[],
  calls: [] as string[],
}));

vi.mock("../../core/plugins/client", () => ({
  pluginClient: {
    call: (method: string) => {
      backend.calls.push(method);
      const next =
        method === "shell-integration.state" ? backend.state.shift() : backend.set.shift();
      if (next === undefined) throw new Error(`unexpected call ${method}`);
      return Promise.resolve(next());
    },
  },
}));

const { t } = await import("../../core/i18n");
const { default: ShellSettings } = await import("./ShellSettings.svelte");

const failure: ApiError = {
  kind: "backend",
  command: "plugin_call",
  error: { kind: "denied", subject: "registry", detail: null },
};
const ok = (value: ShellState) => (): Answer => ({ ok: true, value });
const failed = (): Answer => ({ ok: false, error: failure });
const refused = t("shell.failed", { message: t("error.denied", { subject: "registry" }) });

let component: Record<string, unknown> | null = null;
let root: HTMLElement;

async function open(): Promise<void> {
  root = document.createElement("div");
  document.body.append(root);
  component = mount(ShellSettings, { target: root, props: {} });
  await settle();
}

async function settle(): Promise<void> {
  await vi.waitFor(() => expect(backend.state.length + backend.set.length).toBe(0));
  for (let i = 0; i < 3; i++) {
    await Promise.resolve();
    flushSync();
  }
}

function menuSwitch(): HTMLInputElement {
  const found = root.querySelector<HTMLInputElement>("#set-shell-menu");
  if (found === null) throw new Error("no menu switch");
  return found;
}

afterEach(() => {
  if (component) void unmount(component);
  component = null;
  backend.state.length = 0;
  backend.set.length = 0;
  backend.calls.length = 0;
  document.body.replaceChildren();
});

describe("the file manager card", () => {
  it("a change the backend refused leaves the switch as the registry has it, and says why", async () => {
    backend.state.push(ok({ menu: true, newFile: false }));
    await open();
    expect(menuSwitch().checked).toBe(true);

    backend.set.push(failed);
    backend.state.push(ok({ menu: true, newFile: false }));
    menuSwitch().click();
    expect(menuSwitch().checked).toBe(false);
    await settle();
    expect(backend.calls).toEqual([
      "shell-integration.state",
      "shell-integration.set",
      "shell-integration.state",
    ]);
    expect(menuSwitch().checked).toBe(true);
    expect(root.querySelector('[role="alert"]')?.textContent).toBe(refused);
  });

  it("a first read that failed says why instead of a blank card", async () => {
    backend.state.push(failed);
    await open();
    expect(root.querySelector("#set-shell-menu")).toBeNull();
    expect(root.querySelector('[role="alert"]')?.textContent).toBe(refused);
  });
});
