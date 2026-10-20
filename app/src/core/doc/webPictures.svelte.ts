// Pictures on the web: a chip stands in for each (nothing is fetched while reading); a click on a chip of an `https`
// picture fetches that one picture through the backend, and the person can let a project's web pictures load by
// themselves. The bytes come back raw and show from a `blob:` URL (the CSP allows no web address for pictures).
// Fetched pictures are kept for the window's session, so a block drawn again shows them without a new request.

import type { Api } from "../../api";
import { apiErrorText } from "../errors";
import { t } from "../i18n";

/** Pictures kept per window (oldest let go first). */
export const KEPT = 100;
/** Pictures fetched at the same time when a project loads them by itself. */
export const AT_ONCE = 2;

const CHIP = ".remote-image";

export function isHttps(url: string): boolean {
  return /^https:\/\/[^/?#\s]/i.test(url.trim());
}

class WebPictures {
  /** This project's web pictures load by themselves (the person's choice; never on in a single file). */
  always = $state(false);
  /** The window shows a project (the choice exists). */
  project = $state(false);
  #kept = new Map<string, string>();
  #asking = new Set<string>();
  #queue: Element[] = [];
  #running = 0;

  /** The choice of the project now shown (a single file | the Welcome: off, not offered). */
  async load(api: Api, project: boolean): Promise<void> {
    this.project = project;
    this.always = false;
    this.#queue = [];
    if (!project) return;
    const result = await api.remotePicturesGet();
    if (result.ok) this.always = result.value;
  }

  /** Turns the project's choice on | off; on: the chips of `root` load now. */
  async setAlways(
    api: Api,
    on: boolean,
    root: Element | null,
    note: (text: string) => void,
  ): Promise<void> {
    if (!this.project) return;
    const result = await api.remotePicturesSet(on);
    if (!result.ok) {
      note(apiErrorText(result.error));
      return;
    }
    this.always = on;
    if (on && root) this.sweep(api, root, note);
  }

  /** The chips of `root`: a kept picture shows at once; with the project's choice on, the others are fetched. */
  sweep(api: Api, root: Element, note: (text: string) => void): void {
    for (const chip of Array.from(root.querySelectorAll(CHIP))) {
      const url = chip.getAttribute("data-url") ?? "";
      const kept = this.#kept.get(url);
      if (kept !== undefined) swap(chip, kept, url);
      else if (this.always && isHttps(url) && !this.#asking.has(url)) this.#queue.push(chip);
    }
    this.#pump(api, note);
  }

  /** The person clicked a chip: that one picture is fetched (an address that is not `https` is only told). */
  async show(api: Api, chip: Element, note: (text: string) => void): Promise<void> {
    const url = chip.getAttribute("data-url") ?? "";
    if (!isHttps(url)) {
      note(t("doc.remoteOnlyHttps"));
      return;
    }
    const kept = this.#kept.get(url);
    if (kept !== undefined) {
      swap(chip, kept, url);
      return;
    }
    await this.#fetch(api, chip, url, note);
  }

  /** Lets every kept picture go (tests; a window that closes takes them anyway). */
  forget(): void {
    for (const blob of this.#kept.values()) URL.revokeObjectURL(blob);
    this.#kept.clear();
    this.#queue = [];
  }

  #pump(api: Api, note: (text: string) => void): void {
    while (this.#running < AT_ONCE && this.#queue.length > 0) {
      const chip = this.#queue.shift();
      if (!chip?.isConnected) continue;
      const url = chip.getAttribute("data-url") ?? "";
      this.#running += 1;
      void this.#fetch(api, chip, url, note).finally(() => {
        this.#running -= 1;
        this.#pump(api, note);
      });
    }
  }

  async #fetch(api: Api, chip: Element, url: string, note: (text: string) => void): Promise<void> {
    if (this.#asking.has(url)) return;
    this.#asking.add(url);
    chip.classList.add("is-loading");
    try {
      const result = await api.remotePicture(url);
      if (!result.ok) {
        chip.classList.remove("is-loading");
        note(t("doc.remoteFailed", { reason: apiErrorText(result.error) }));
        return;
      }
      const blob = URL.createObjectURL(new Blob([result.value]));
      this.#keep(url, blob);
      // the page may have drawn the block again meanwhile: every chip of that address takes the picture
      if (chip.isConnected) swap(chip, blob, url);
      for (const other of Array.from(document.querySelectorAll(CHIP))) {
        if (other.getAttribute("data-url") === url) swap(other, blob, url);
      }
    } finally {
      this.#asking.delete(url);
    }
  }

  #keep(url: string, blob: string): void {
    this.#kept.set(url, blob);
    while (this.#kept.size > KEPT) {
      const [oldest, old] = this.#kept.entries().next().value ?? [];
      if (oldest === undefined || old === undefined) break;
      this.#kept.delete(oldest);
      URL.revokeObjectURL(old);
    }
  }
}

/** The picture in the chip's place, with the chip's description. */
function swap(chip: Element, blob: string, url: string): void {
  const img = document.createElement("img");
  img.src = blob;
  img.alt = chip.getAttribute("aria-label") ?? "";
  img.title = url;
  img.className = "remote-loaded";
  img.setAttribute("data-url", url);
  chip.replaceWith(img);
}

export const webPictures = new WebPictures();
