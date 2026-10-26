// @vitest-environment jsdom
// The quiet notice for a state file PerMark could not read: set aside (a backup) or left in place (none).
import { flushSync, mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { createMockApi } from "../../api/mock";
import type { Api } from "../../api";
import en from "../i18n/locales/en";
import { WindowStore } from "../window.svelte";
import Notices from "./Notices.svelte";

const FILE = "C:/Users/demo/AppData/Roaming/PerMark/session.json";

async function bootedWith(backup: string | null): Promise<WindowStore> {
  const mock = createMockApi();
  const api: Api = {
    ...mock,
    windowBoot: async () => {
      const booted = await mock.windowBoot();
      return booted.ok
        ? { ok: true, value: { ...booted.value, notices: [{ file: FILE, backup }] } }
        : booted;
    },
  };
  const store = new WindowStore(api);
  await store.start();
  return store;
}

let component: Record<string, unknown> | null = null;
afterEach(async () => {
  if (component) await unmount(component);
  component = null;
  document.body.innerHTML = "";
});

describe("a state file that could not be read", () => {
  it("set aside: says so", async () => {
    const store = await bootedWith(FILE + ".bad");
    const target = document.body.appendChild(document.createElement("div"));
    component = mount(Notices, { target, props: { store } });
    flushSync();
    expect(target.textContent).toContain(en["notice.stateRecovered"].replace("{file}", FILE));
  });

  it("left in place (no backup): says it was kept and not saved over, never that it was set aside", async () => {
    const store = await bootedWith(null);
    const target = document.body.appendChild(document.createElement("div"));
    component = mount(Notices, { target, props: { store } });
    flushSync();
    expect(target.textContent).toContain(en["notice.stateUnreadable"].replace("{file}", FILE));
    expect(target.textContent).not.toContain(en["notice.stateRecovered"].replace("{file}", FILE));
  });
});
