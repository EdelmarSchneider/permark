// @vitest-environment jsdom
// Web pictures: nothing is fetched while reading; a click fetches one picture; the project's choice loads the rest.

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { Api, Result } from "../../api";
import { AT_ONCE, isHttps, webPictures } from "./webPictures.svelte";

type Fake = {
  api: Api;
  asked: string[];
  release: () => void;
  saved: boolean[];
};

/** An API whose picture answers wait until `release` (to see how many run at once). */
function fake(options: { fail?: boolean; hold?: boolean; always?: boolean } = {}): Fake {
  const asked: string[] = [];
  const saved: boolean[] = [];
  const waiting: (() => void)[] = [];
  const api = {
    remotePicture: (url: string): Promise<Result<Uint8Array<ArrayBuffer>>> => {
      asked.push(url);
      const answer = (): Result<Uint8Array<ArrayBuffer>> =>
        options.fail
          ? {
              ok: false,
              error: {
                kind: "backend",
                command: "remote_picture",
                error: { kind: "invalidData", subject: null, detail: "picture" },
              },
            }
          : { ok: true, value: new Uint8Array([0x89, 0x50, 0x4e, 0x47]) };
      if (!options.hold) return Promise.resolve(answer());
      return new Promise((resolve) => waiting.push(() => resolve(answer())));
    },
    remotePicturesGet: () => Promise.resolve({ ok: true, value: options.always === true }),
    remotePicturesSet: (always: boolean) => {
      saved.push(always);
      return Promise.resolve({ ok: true, value: null });
    },
  } as unknown as Api;
  return { api, asked, saved, release: () => waiting.splice(0).forEach((go) => go()) };
}

function page(urls: string[]): HTMLElement {
  const root = document.createElement("div");
  for (const url of urls) {
    const chip = document.createElement("span");
    chip.className = "remote-image";
    chip.setAttribute("data-url", url);
    chip.setAttribute("aria-label", `alt of ${url}`);
    root.append(chip);
  }
  document.body.append(root);
  return root;
}

const flush = async (): Promise<void> => {
  for (let i = 0; i < 5; i++) await Promise.resolve();
};

let made = 0;
beforeEach(() => {
  made = 0;
  vi.stubGlobal("URL", {
    ...URL,
    createObjectURL: () => `blob:test/${++made}`,
    revokeObjectURL: vi.fn(),
  });
});
afterEach(() => {
  webPictures.forget();
  document.body.innerHTML = "";
  vi.unstubAllGlobals();
});

describe("web pictures", () => {
  it("fetch nothing while reading, tracking pixels included, when the project's choice is off", async () => {
    const f = fake();
    await webPictures.load(f.api, true);
    const root = page(["https://t.example/pixel.gif", "https://e.example/a.png"]);
    webPictures.sweep(f.api, root, () => {});
    await flush();
    expect(f.asked).toEqual([]);
    expect(root.querySelectorAll(".remote-image")).toHaveLength(2);
  });

  it("a click fetches that one picture and shows it with its description", async () => {
    const f = fake();
    await webPictures.load(f.api, true);
    const root = page(["https://e.example/a.png", "https://e.example/b.png"]);
    const chip = root.querySelector(".remote-image");
    if (!chip) throw new Error("no chip");
    await webPictures.show(f.api, chip, () => {});
    expect(f.asked).toEqual(["https://e.example/a.png"]);
    const img = root.querySelector("img.remote-loaded");
    expect(img?.getAttribute("src")).toBe("blob:test/1");
    expect(img?.getAttribute("alt")).toBe("alt of https://e.example/a.png");
    expect(root.querySelectorAll(".remote-image")).toHaveLength(1);
  });

  it("an address that is not https is never asked; a failure is told and the chip stays", async () => {
    const notes: string[] = [];
    const f = fake({ fail: true });
    await webPictures.load(f.api, true);
    const root = page(["http://e.example/a.png", "javascript:alert(1)", "https://e.example/x"]);
    const chips = [...root.querySelectorAll(".remote-image")];
    for (const chip of chips) await webPictures.show(f.api, chip, (t) => notes.push(t));
    expect(f.asked).toEqual(["https://e.example/x"]);
    expect(notes).toHaveLength(3);
    expect(root.querySelectorAll(".remote-image")).toHaveLength(3);
    expect(root.querySelector(".is-loading")).toBeNull();
  });

  it("a picture drawn again comes back from the window's memory, without a new request", async () => {
    const f = fake();
    await webPictures.load(f.api, true);
    const first = page(["https://e.example/a.png"]);
    const chip = first.querySelector(".remote-image");
    if (!chip) throw new Error("no chip");
    await webPictures.show(f.api, chip, () => {});
    const again = page(["https://e.example/a.png"]);
    webPictures.sweep(f.api, again, () => {});
    expect(again.querySelector("img.remote-loaded")).not.toBeNull();
    expect(f.asked).toHaveLength(1);
  });

  it("with the project's choice on, https pictures load by themselves, two at a time", async () => {
    const f = fake({ hold: true, always: true });
    await webPictures.load(f.api, true);
    expect(webPictures.always).toBe(true);
    const urls = ["a", "b", "c", "d", "e"].map((n) => `https://e.example/${n}.png`);
    const root = page([...urls, "http://e.example/plain.png"]);
    webPictures.sweep(f.api, root, () => {});
    await flush();
    expect(f.asked).toHaveLength(AT_ONCE);
    for (let i = 0; i < 5; i++) {
      f.release();
      await flush();
    }
    expect([...f.asked].sort()).toEqual(urls);
    expect(root.querySelectorAll("img.remote-loaded")).toHaveLength(5);
    expect(root.querySelectorAll(".remote-image")).toHaveLength(1);
  });

  it("the choice is the project's; a single file neither reads nor writes it", async () => {
    const f = fake({ always: true });
    await webPictures.load(f.api, false);
    expect(webPictures.always).toBe(false);
    await webPictures.setAlways(f.api, true, null, () => {});
    expect(f.saved).toEqual([]);
    await webPictures.load(f.api, true);
    await webPictures.setAlways(f.api, false, null, () => {});
    expect(f.saved).toEqual([false]);
    expect(webPictures.always).toBe(false);
  });

  it("knows an https address", () => {
    expect(isHttps("https://e.example/a.png")).toBe(true);
    expect(isHttps("HTTPS://e.example/a.png")).toBe(true);
    for (const url of [
      "http://e.example",
      "https://",
      "https:///x",
      "//e.example/a",
      "data:x",
      "",
    ]) {
      expect(isHttps(url), url).toBe(false);
    }
  });
});
