// One export page for a Markdown document: the reading view's blocks drawn off screen with every embed finished
// (code colors, formulas, diagrams), then copied into a page that stands alone: pictures, sound, video and fonts
// inside as data URIs, links that lead out of the project only, no script, no app controls, the reading view's
// styles. PDF and print load it in a hidden window with the paper look; HTML is saved as it is.

import type { Api, ApiError } from "../../api";
import type { PageBox } from "../../api/generated/PageBox";
import { conditions } from "../../core/conditions/context.svelte";
import { Decorator, type EmbedLookup } from "../../core/doc/decorate";
import { enhanceBlock, type EnhanceEnv } from "../../core/doc/enhance";
import { webPictures } from "../../core/doc/webPictures.svelte";
import { contributions } from "../../core/plugins/instance.svelte";
import { fetchDataUri } from "./assets";
import { exportCss } from "./look";

export type ExportTarget = "paper" | "html";

/** Something of the document the page could not carry (the person is told). */
export type ExportNote = { kind: "picture" | "webPicture" | "video" | "audio"; name: string };

/** The words the page itself holds where a file was left out (the caller passes them translated). */
export type ExportWords = {
  videoLeftOut(name: string): string;
  audioLeftOut(name: string): string;
  diagramFailed: string;
};

export const ENGLISH_WORDS: ExportWords = {
  videoLeftOut: (name) => `Video left out: ${name}`,
  audioLeftOut: (name) => `Sound left out: ${name}`,
  diagramFailed: "This diagram could not be drawn.",
};

export type ExportOptions = {
  /** `paper`: light look and the page rule (PDF, print); `html`: a page that follows the reader's light | dark. */
  target: ExportTarget;
  /** The paper's page box (paper only; absent: the web view's own page). */
  page?: PageBox;
  /** "n / total" at the bottom of each page (paper; only where the web view draws page boxes). */
  pageNumbers: boolean;
  /** The web view puts the page box's margins itself (paper): the page rule asks for none of its own. */
  nativeMargins?: boolean;
  /** The reading view's enhance environment (where the window loads project files from). */
  env: EnhanceEnv;
  /** The embeds on offer; absent: the ones the reading view uses now. */
  embeds?: EmbedLookup;
  words?: ExportWords;
  signal?: AbortSignal;
  /** Longest wait for embeds to finish drawing; a diagram not drawn by then shows the error. */
  settleMs?: number;
};

export type ExportOutcome =
  | { status: "done"; html: string; notes: ExportNote[] }
  | { status: "failed"; error: ApiError }
  | { status: "notMarkdown" }
  | { status: "cancelled" };

const CSP =
  "default-src 'none'; img-src data:; media-src data:; font-src data:; style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'";
/** Blocks built (and embeds started) per turn: the window keeps answering during a long document. */
const SLICE = 200;
const SETTLE_MS = 60_000;
/** The page counts as drawn once nothing in it changed for this long (and no embed says it is busy). */
const QUIET_MS = 150;
const POLL_MS = 30;
/** Embeds that say they are still drawing: a diagram's figure, a code block waiting for its colors. */
const BUSY = ".figure.is-pending, [aria-busy='true']";
const KEPT_LINK = /^(?:https?:|mailto:|#)/i;
/**
 * App-only marks and controls, and elements that load | send | redirect on their own: a static page has no use for
 * them (the page's own head tags are added after cleaning).
 */
const APP_ONLY = "script, button, a.anchor, iframe, object, embed, form, base, link, meta";
const APP_ATTRS = new Set([
  "tabindex",
  "contenteditable",
  "draggable",
  "aria-busy",
  "inert",
  "loading",
  "decoding",
]);

const cancelled: ExportOutcome = { status: "cancelled" };

function pageEmbeds(language: string): ReturnType<EmbedLookup> {
  return contributions.visible("embed", conditions.current).find((e) => e.language === language)
    ?.view;
}

function turn(signal: AbortSignal | undefined, ms = 0): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms)).then(() => signal?.throwIfAborted());
}

/** An off-screen copy of the page, laid out at the width the output has, in one theme. */
class Stage {
  readonly host: HTMLElement;
  readonly body: HTMLElement;

  constructor(theme: "light" | "dark", width: string, paper: boolean) {
    this.host = document.createElement("div");
    this.host.className = "pm-export-stage";
    this.host.setAttribute("aria-hidden", "true");
    this.host.setAttribute("inert", "");
    this.host.dataset["theme"] = theme;
    this.host.style.cssText = `position: fixed; top: 0; left: -100000px; width: ${width}; pointer-events: none;`;
    this.body = document.createElement("main");
    this.body.className = "md md-body";
    if (paper) this.body.style.maxWidth = "none";
    this.host.append(this.body);
    document.body.append(this.host);
  }
}

/** Settles once `work` does, `signal` aborts or `until` passes, whichever comes first (`work` may never settle). */
function within(
  work: Promise<unknown>,
  until: number,
  signal: AbortSignal | undefined,
): Promise<void> {
  return new Promise((resolve) => {
    const finish = (): void => {
      clearTimeout(timer);
      signal?.removeEventListener("abort", finish);
      resolve();
    };
    const timer = setTimeout(finish, Math.max(0, until - Date.now()));
    signal?.addEventListener("abort", finish, { once: true });
    work.then(finish, finish);
  });
}

/** Waits until no embed of `roots` is busy and nothing changed for a moment, or until `until`. */
async function settle(roots: HTMLElement[], until: number, signal?: AbortSignal): Promise<void> {
  let changed = Date.now();
  const observer = new MutationObserver(() => {
    changed = Date.now();
  });
  for (const root of roots) {
    observer.observe(root, {
      subtree: true,
      childList: true,
      characterData: true,
      attributes: true,
    });
  }
  try {
    while (Date.now() < until) {
      const busy = roots.some((root) => root.querySelector(BUSY) !== null);
      if (!busy && Date.now() - changed >= QUIET_MS) return;
      await turn(signal, POLL_MS);
    }
  } finally {
    observer.disconnect();
  }
}

const nameOf = (path: string): string => path.split("/").pop() ?? path;

/** Builds the export page of the Markdown document at `path` (the saved file, as the reading view reads it). */
export async function buildExport(
  api: Api,
  path: string,
  options: ExportOptions,
): Promise<ExportOutcome> {
  const { signal } = options;
  const result = await api.openDocument(path, []);
  if (signal?.aborted) return cancelled;
  if (!result.ok) return { status: "failed", error: result.error };
  const view = result.value;
  if (view.body.kind !== "markdown") return { status: "notMarkdown" };

  // the window's asset URLs, with the project path each stands for (names for the notes)
  const files = new Map<string, string>();
  const env: EnhanceEnv = {
    assetUrl: (file) => {
      const url = options.env.assetUrl(file);
      files.set(url, file);
      return url;
    },
  };
  const paper = options.target === "paper";
  const width =
    paper && options.page
      ? `${options.page.widthMm - 2 * options.page.marginMm}mm`
      : "var(--measure)";
  const lookup = options.embeds ?? pageEmbeds;
  const stages: Stage[] = [];
  const decorators: Decorator[] = [];
  try {
    const light = new Stage("light", width, paper);
    stages.push(light);
    const template = document.createElement("template");
    for (const [at, block] of view.body.blocks.entries()) {
      if (block.html === null) continue;
      template.innerHTML = block.html;
      const first = template.content.firstElementChild;
      if (first instanceof HTMLElement) light.body.append(enhanceBlock(first, env));
      if (at % SLICE === SLICE - 1) await turn(signal);
    }
    signal?.throwIfAborted();

    // HTML follows the reader's theme: each diagram is drawn a second time, dark, off a copy of its block
    const pairs: HTMLElement[] = [];
    if (!paper) {
      for (const holder of light.body.querySelectorAll<HTMLElement>(".code-block")) {
        const info = (holder.getAttribute("data-info") ?? "").toLowerCase();
        if (info !== "" && lookup(info) !== undefined) pairs.push(holder);
      }
    }
    const dark = pairs.length > 0 ? new Stage("dark", width, paper) : null;
    if (dark) {
      stages.push(dark);
      pairs.forEach((holder, at) => {
        holder.dataset["pmPair"] = String(at);
        dark.body.append(holder.cloneNode(true));
      });
    }

    for (const stage of stages) {
      const decorator = new Decorator(api, { theme: stage === dark ? "dark" : "light" });
      decorators.push(decorator);
      decorator.setEmbeds(lookup);
      const blocks = Array.from(stage.body.children).filter((el) => el instanceof HTMLElement);
      for (const [at, block] of blocks.entries()) {
        decorator.near(block);
        if (at % SLICE === SLICE - 1) await turn(signal);
      }
    }
    // one deadline for loading and drawing: a library import that never ends holds neither Cancel nor the build
    const until = Date.now() + (options.settleMs ?? SETTLE_MS);
    await within(Promise.all(decorators.map((decorator) => decorator.idle())), until, signal);
    signal?.throwIfAborted();
    await settle(
      stages.map((stage) => stage.body),
      until,
      signal,
    );
    signal?.throwIfAborted();
    if (paper) markWideTables(light.body);

    const page = light.body.cloneNode(true);
    if (!(page instanceof HTMLElement)) return cancelled;
    if (dark) pairDiagrams(page, dark.body);
    const words = options.words ?? ENGLISH_WORDS;
    clean(page, words);

    const notes: ExportNote[] = [];
    await inlinePictures(page, files, notes, signal);
    await inlineMedia(page, files, options.target, words, notes, signal);
    signal?.throwIfAborted();

    const css = await exportCss(page, options);
    signal?.throwIfAborted();
    dropAttributes(page, css);
    return {
      status: "done",
      html: serialize(page, css, {
        title: view.name.replace(/\.[^.]+$/, ""),
        lang: document.documentElement.lang,
        paper,
      }),
      notes,
    };
  } catch (error) {
    if (signal?.aborted) return cancelled;
    throw error;
  } finally {
    for (const decorator of decorators) decorator.dispose();
    for (const stage of stages) stage.host.remove();
  }
}

/** A table wider than the printable width gets a smaller font on paper. */
function markWideTables(root: HTMLElement): void {
  for (const wrap of root.querySelectorAll<HTMLElement>(".table-wrap")) {
    if (wrap.scrollWidth > wrap.clientWidth + 1) wrap.classList.add("pm-wide-table");
  }
}

/** Puts each dark drawing after its light one; the page's styles show one by the reader's preference. */
function pairDiagrams(page: HTMLElement, dark: HTMLElement): void {
  for (const holder of page.querySelectorAll<HTMLElement>("[data-pm-pair]")) {
    const at = holder.dataset["pmPair"] ?? "";
    const lightFigure = holder.querySelector("figure.figure:not(.is-failed)");
    const darkFigure = dark
      .querySelector(`[data-pm-pair="${at}"]`)
      ?.querySelector("figure.figure:not(.is-failed)");
    if (!lightFigure || !darkFigure) continue;
    const copy = darkFigure.cloneNode(true);
    if (!(copy instanceof Element)) continue;
    lightFigure.classList.add("pm-only-light");
    copy.classList.add("pm-only-dark");
    lightFigure.after(copy);
  }
}

/** Takes out what only works in the app: controls, scripts, links into the project, diagram sources. */
function clean(page: HTMLElement, words: ExportWords): void {
  for (const element of page.querySelectorAll(APP_ONLY)) element.remove();
  // a diagram that was not drawn says so; its source never stands in for the drawing
  for (const figure of page.querySelectorAll(".figure.is-failed, .figure.is-pending")) {
    const message = figure.querySelector(".message");
    figure.classList.remove("is-pending");
    figure.classList.add("is-failed");
    if (message) {
      figure.replaceChildren(message);
    } else {
      const caption = document.createElement("figcaption");
      caption.className = "message";
      caption.textContent = words.diagramFailed;
      figure.replaceChildren(caption);
    }
  }
  for (const link of page.querySelectorAll("a")) {
    const href = link.getAttribute("href")?.trim() ?? "";
    if (!KEPT_LINK.test(href)) link.replaceWith(...link.childNodes);
  }
}

/** A picture that could not be read: its description (else its name) stands in its place. */
function standIn(img: Element, name: string): void {
  img.replaceWith(document.createTextNode(img.getAttribute("alt") || name));
}

async function decodes(img: HTMLImageElement): Promise<boolean> {
  if (typeof img.decode !== "function") return true;
  try {
    await img.decode();
    return true;
  } catch {
    return false;
  }
}

async function inlinePictures(
  page: HTMLElement,
  files: ReadonlyMap<string, string>,
  notes: ExportNote[],
  signal: AbortSignal | undefined,
): Promise<void> {
  for (const img of page.querySelectorAll("img")) {
    const src = img.getAttribute("src") ?? "";
    const file = files.get(src);
    const name = nameOf(file ?? src);
    // only the window's own project files (and pictures already inside the page) are read
    const got =
      file !== undefined || src.startsWith("data:")
        ? await fetchDataUri(src, { signal })
        : ({ status: "failed" } as const);
    if (got.status === "ok") {
      img.setAttribute("src", got.uri);
      if (await decodes(img)) continue;
    }
    standIn(img, name);
    notes.push({ kind: "picture", name });
  }
  // pictures on the web are never fetched for an export: only one this window already holds comes in
  for (const chip of page.querySelectorAll(".remote-image")) {
    const url = chip.getAttribute("data-url") ?? "";
    const kept = webPictures.kept(url);
    const got = kept === undefined ? null : await fetchDataUri(kept, { signal });
    if (got?.status === "ok") {
      const img = document.createElement("img");
      img.src = got.uri;
      img.alt = chip.getAttribute("aria-label") ?? "";
      chip.replaceWith(img);
      continue;
    }
    chip.removeAttribute("data-url");
    chip.removeAttribute("title");
    notes.push({ kind: "webPicture", name: url });
  }
}

/** A sound | video: inside the page (HTML, up to the limit), else its still and | or a note in its place. */
async function inlineMedia(
  page: HTMLElement,
  files: ReadonlyMap<string, string>,
  target: ExportTarget,
  words: ExportWords,
  notes: ExportNote[],
  signal: AbortSignal | undefined,
): Promise<void> {
  for (const media of page.querySelectorAll("video, audio")) {
    const kind = media.localName === "video" ? "video" : "audio";
    const sources = [media, ...media.querySelectorAll("source")].filter((el) =>
      el.hasAttribute("src"),
    );
    const first = sources[0]?.getAttribute("src") ?? "";
    const name = nameOf(files.get(first) ?? first);
    const poster = media.getAttribute("poster");
    const still =
      kind === "video" && poster !== null && files.has(poster)
        ? await fetchDataUri(poster, { signal })
        : null;
    if (target === "html" && sources.length > 0) {
      let inside = true;
      for (const source of sources) {
        const src = source.getAttribute("src") ?? "";
        const got = files.has(src) ? await fetchDataUri(src, { signal }) : null;
        if (got?.status !== "ok") {
          inside = false;
          break;
        }
        source.setAttribute("src", got.uri);
      }
      if (inside) {
        if (still?.status === "ok") media.setAttribute("poster", still.uri);
        else media.removeAttribute("poster");
        continue;
      }
    }
    const img = document.createElement("img");
    if (still?.status === "ok") {
      img.src = still.uri;
      img.alt = name;
      // paper shows a video's still on its own; a page that could not carry the video says so beside it
      if (target === "paper") {
        media.replaceWith(img);
        continue;
      }
    }
    const note = document.createElement("span");
    note.className = "pm-left-out";
    if (still?.status === "ok") note.append(img);
    note.append(kind === "video" ? words.videoLeftOut(name) : words.audioLeftOut(name));
    media.replaceWith(note);
    notes.push({ kind, name });
  }
}

/** Takes out event handlers, app attributes and every `data-*` attribute the page's styles do not use. */
function dropAttributes(page: HTMLElement, css: string): void {
  const styled = new Set(Array.from(css.matchAll(/\[(data-[\w-]+)/g), (match) => match[1]));
  for (const element of page.querySelectorAll("*")) {
    for (const { name } of Array.from(element.attributes)) {
      const lower = name.toLowerCase();
      if (
        lower.startsWith("on") ||
        APP_ATTRS.has(lower) ||
        (lower.startsWith("data-") && !styled.has(lower))
      ) {
        element.removeAttribute(name);
      }
    }
  }
}

function serialize(
  page: HTMLElement,
  css: string,
  head: { title: string; lang: string; paper: boolean },
): string {
  const doc = document.implementation.createHTMLDocument(head.title);
  const root = doc.documentElement;
  if (head.lang !== "") root.setAttribute("lang", head.lang);
  if (head.paper) {
    root.setAttribute("data-theme", "light");
    root.setAttribute("class", "pm-paper");
  }
  const meta = (attrs: Record<string, string>): HTMLMetaElement => {
    const element = doc.createElement("meta");
    for (const [key, value] of Object.entries(attrs)) element.setAttribute(key, value);
    return element;
  };
  const style = doc.createElement("style");
  // a style element ends at the first `</style`: no text of the styles may close it early
  style.textContent = css.replace(/<\/style/gi, "<\\/style");
  doc.head.prepend(
    meta({ charset: "utf-8" }),
    meta({ name: "viewport", content: "width=device-width, initial-scale=1" }),
    meta({ name: "color-scheme", content: head.paper ? "light" : "light dark" }),
    meta({ "http-equiv": "Content-Security-Policy", content: CSP }),
  );
  doc.head.append(style);
  const main = doc.createElement("main");
  main.className = "md md-body";
  main.innerHTML = page.innerHTML;
  doc.body.append(main);
  return `<!doctype html>${root.outerHTML}`;
}
