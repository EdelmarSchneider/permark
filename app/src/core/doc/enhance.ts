// Structure added to a rendered block right after it is created, before it is shown: code and table wrappers, heading
// anchors, alert titles, task marks, asset URLs, and a quiet stand-in for pictures that live on the web. Synchronous and
// cheap; heavy things (highlighting, math, diagrams) come later, when the block is near the screen.
// Returns the block's root: a `pre` or `table` that is a block of its own is wrapped, so the wrapper is the block.

import { t, type MessageKey } from "../i18n";

/** The kind of block, as the wrapper tells it. */
export const CODE_BLOCK = "code-block";
export const TABLE_WRAP = "table-wrap";

export type EnhanceEnv = {
  /** URL the webview loads a project file from. */
  assetUrl(path: string): string;
};

const SVG =
  'xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"';

/** Each kind of callout its icon (lucide's, as Obsidian's callouts use them). */
const ALERT_ICONS = {
  note: `<svg ${SVG}><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`,
  tip: `<svg ${SVG}><path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5"/><path d="M9 18h6"/><path d="M10 22h4"/></svg>`,
  important: `<svg ${SVG}><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/><path d="M12 7v2"/><path d="M12 13h.01"/></svg>`,
  warning: `<svg ${SVG}><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/></svg>`,
  caution: `<svg ${SVG}><path d="M12 16h.01"/><path d="M12 8v4"/><path d="M15.312 2a2 2 0 0 1 1.414.586l4.688 4.688A2 2 0 0 1 22 8.688v6.624a2 2 0 0 1-.586 1.414l-4.688 4.688a2 2 0 0 1-1.414.586H8.688a2 2 0 0 1-1.414-.586l-4.688-4.688A2 2 0 0 1 2 15.312V8.688a2 2 0 0 1 .586-1.414l4.688-4.688A2 2 0 0 1 8.688 2z"/></svg>`,
  info: `<svg ${SVG}><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`,
  todo: `<svg ${SVG}><circle cx="12" cy="12" r="10"/><path d="m16 9-5.5 5.5L8 12"/></svg>`,
  abstract: `<svg ${SVG}><rect width="8" height="4" x="8" y="2" rx="1" ry="1"/><path d="M16 4h2a2 2 0 0 1 2 2v14a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2V6a2 2 0 0 1 2-2h2"/><path d="M12 11h4"/><path d="M12 16h4"/><path d="M8 11h.01"/><path d="M8 16h.01"/></svg>`,
  success: `<svg ${SVG}><path d="M20 6 9 17l-5-5"/></svg>`,
  question: `<svg ${SVG}><circle cx="12" cy="12" r="10"/><path d="M9.09 9a3 3 0 0 1 5.83 1c0 2-3 3-3 3"/><path d="M12 17h.01"/></svg>`,
  example: `<svg ${SVG}><path d="M3 5h.01"/><path d="M3 12h.01"/><path d="M3 19h.01"/><path d="M8 5h13"/><path d="M8 12h13"/><path d="M8 19h13"/></svg>`,
  failure: `<svg ${SVG}><path d="M18 6 6 18"/><path d="m6 6 12 12"/></svg>`,
  danger: `<svg ${SVG}><path d="M15.914 4a1.5 1.5 0 00-2.474-1.561l-9 9A1.5 1.5 0 005.5 14h4.002a.5.5 0 01.471.666L8.086 20a1.5 1.5 0 002.475 1.56l9-9A1.5 1.5 0 0018.5 10h-3.997a.5.5 0 01-.472-.667z"/></svg>`,
  bug: `<svg ${SVG}><path d="M12 20v-9"/><path d="M14 7a4 4 0 0 1 4 4v3a6 6 0 0 1-12 0v-3a4 4 0 0 1 4-4z"/><path d="M14.12 3.88 16 2"/><path d="M21 21a4 4 0 0 0-3.81-4"/><path d="M21 5a4 4 0 0 1-3.55 3.97"/><path d="M22 13h-4"/><path d="M3 21a4 4 0 0 1 3.81-4"/><path d="M3 5a4 4 0 0 0 3.55 3.97"/></svg>`,
  quote: `<svg ${SVG}><path d="M16 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2z"/><path d="M5 3a2 2 0 0 0-2 2v6a2 2 0 0 0 2 2 1 1 0 0 1 1 1v1a2 2 0 0 1-2 2 1 1 0 0 0-1 1v2a1 1 0 0 0 1 1 6 6 0 0 0 6-6V5a2 2 0 0 0-2-2z"/></svg>`,
  details: `<svg ${SVG}><path d="m7 20 5-5 5 5"/><path d="m7 4 5 5 5-5"/></svg>`,
} as const;

type AlertKind = keyof typeof ALERT_ICONS;

/**
 * Per kind, the title the renderer writes when the author gave none (comrak's for GitHub's five kinds, the
 * backend's callout table for the others) and its key. Only that title is translated.
 */
const ALERT_TITLES: Record<AlertKind, { english: string; key: MessageKey }> = {
  note: { english: "Note", key: "alert.note" },
  tip: { english: "Tip", key: "alert.tip" },
  important: { english: "Important", key: "alert.important" },
  warning: { english: "Warning", key: "alert.warning" },
  caution: { english: "Caution", key: "alert.caution" },
  info: { english: "Info", key: "alert.info" },
  todo: { english: "To do", key: "alert.todo" },
  abstract: { english: "Abstract", key: "alert.abstract" },
  success: { english: "Success", key: "alert.success" },
  question: { english: "Question", key: "alert.question" },
  example: { english: "Example", key: "alert.example" },
  failure: { english: "Failure", key: "alert.failure" },
  danger: { english: "Danger", key: "alert.danger" },
  bug: { english: "Bug", key: "alert.bug" },
  quote: { english: "Quote", key: "alert.quote" },
  details: { english: "Details", key: "alert.details" },
};

function isAlertKind(kind: string): kind is AlertKind {
  return Object.hasOwn(ALERT_ICONS, kind);
}

const LOOK_CLASS = "markdown-alert-";
const KIND_CLASS = "markdown-alert-kind-";

/** A callout's kind from its classes: `markdown-alert-kind-x` when given, else its look `markdown-alert-x`. */
function alertKind(alert: Element | null): AlertKind {
  const names = [...(alert?.classList ?? [])];
  const kind =
    names.find((name) => name.startsWith(KIND_CLASS))?.slice(KIND_CLASS.length) ??
    names.find((name) => name.startsWith(LOOK_CLASS))?.slice(LOOK_CLASS.length);
  return kind !== undefined && isAlertKind(kind) ? kind : "note";
}

/** The extras of a code block's info string the renderer checked (on `pre > code`), handed to its wrapper. */
const CODE_EXTRAS = ["data-title", "data-lines", "data-numbers"] as const;

const SELECTOR =
  "pre, table, h1, h2, h3, h4, h5, h6, .markdown-alert-title, a.footnote-backref, li > input[type=checkbox], img, video, audio, source";

const PREFIX = "pm-asset:";
/** An address on the web: what View never fetches by itself. */
export const WEB_ADDRESS = /^(?:https?:)?\/\//i;

/** The path inside a `pm-asset:` URL (decoded), or null for any other URL. */
export function assetPathOf(url: string): string | null {
  if (!url.startsWith(PREFIX)) return null;
  try {
    return decodeURIComponent(url.slice(PREFIX.length));
  } catch {
    return null;
  }
}

/** The language word of a code block (`language-rust` -> `rust`), or "". */
export function languageOf(pre: Element): string {
  const code = pre.querySelector("code");
  const token = (code?.getAttribute("class") ?? "")
    .split(/\s+/)
    .find((name) => name.startsWith("language-"));
  return token === undefined ? "" : token.slice("language-".length);
}

function wrap(element: HTMLElement, className: string): HTMLElement {
  const wrapper = document.createElement("div");
  wrapper.className = className;
  // a block of its own passes its mark to the wrapper
  const mark = element.getAttribute("data-b");
  if (mark !== null) {
    wrapper.setAttribute("data-b", mark);
    element.removeAttribute("data-b");
  }
  element.replaceWith(wrapper);
  wrapper.append(element);
  return wrapper;
}

function isImageOnly(paragraph: HTMLElement): boolean {
  let images = 0;
  for (const node of paragraph.childNodes) {
    if (node instanceof HTMLImageElement) images += 1;
    else if (node instanceof HTMLBRElement) continue;
    else if (node.nodeType === Node.TEXT_NODE && (node.textContent ?? "").trim() === "") continue;
    else return false;
  }
  return images > 0;
}

/**
 * The stand-in for the web picture at `url`, its address as its tooltip. View's chips also carry `data-url` and say how
 * to load them (a click | the project's choice fetch them and put the picture in the chip's place); Edit's go without:
 * the editor owns their place.
 */
export function remoteChip(alt: string, url: string): HTMLElement {
  const chip = document.createElement("span");
  chip.className = "remote-image";
  chip.setAttribute("role", "img");
  chip.setAttribute("aria-label", alt);
  chip.title = url;
  chip.textContent = alt === "" ? t("doc.remoteImageNoAlt") : alt;
  return chip;
}

/** View's chip: loads on a click. */
function loadableChip(alt: string, url: string): HTMLElement {
  const chip = remoteChip(alt, url);
  chip.setAttribute("data-url", url);
  chip.title = t("doc.remoteImage", { url });
  return chip;
}

/** Longest accessible name of a task checkbox (a long item is cut; the item itself is read next anyway). */
const TASK_NAME_MAX = 120;

/** One line, cut long, from the text pieces of a task (the pieces must already drop nested lists). */
export function taskLabel(pieces: string[]): string {
  const line = pieces.join(" ").replace(/\s+/g, " ").trim();
  return line.length > TASK_NAME_MAX ? `${line.slice(0, TASK_NAME_MAX - 1).trimEnd()}…` : line;
}

/** The text of a task item without its nested lists, on one line. */
export function taskName(item: Element): string {
  const pieces: string[] = [];
  for (const node of item.childNodes) {
    if (node instanceof Element && (node.localName === "ul" || node.localName === "ol")) continue;
    pieces.push(node.textContent ?? "");
  }
  return taskLabel(pieces);
}

function enhanceOne(element: HTMLElement, env: EnhanceEnv): HTMLElement {
  let result = element;
  switch (element.localName) {
    case "pre": {
      const wrapper = wrap(element, CODE_BLOCK);
      wrapper.setAttribute("data-info", languageOf(element));
      const code = element.querySelector("code");
      for (const name of CODE_EXTRAS) {
        const value = code?.getAttribute(name);
        if (value) wrapper.setAttribute(name, value);
      }
      result = wrapper;
      break;
    }
    case "table":
      result = wrap(element, TABLE_WRAP);
      break;
    case "h1":
    case "h2":
    case "h3":
    case "h4":
    case "h5":
    case "h6": {
      const id = element.id;
      if (id === "" || element.querySelector(":scope > a.anchor")) break;
      const anchor = document.createElement("a");
      anchor.className = "anchor";
      anchor.setAttribute("href", `#${id}`);
      anchor.setAttribute(
        "aria-label",
        t("doc.anchorLabel", { name: (element.textContent ?? "").trim() }),
      );
      anchor.textContent = "#";
      element.prepend(anchor);
      break;
    }
    case "img": {
      const src = element.getAttribute("src") ?? "";
      const path = assetPathOf(src);
      if (path !== null) {
        element.setAttribute("src", env.assetUrl(path));
        element.setAttribute("loading", "lazy");
        element.setAttribute("decoding", "async");
      } else if (WEB_ADDRESS.test(src)) {
        // pictures on the web are not fetched (offline by design); a chip says what was there
        const chip = loadableChip(element.getAttribute("alt") ?? "", src);
        element.replaceWith(chip);
        result = element === result ? chip : result;
      }
      break;
    }
    case "video":
    case "audio":
    case "source": {
      for (const attr of ["src", "poster"]) {
        const value = element.getAttribute(attr);
        if (value === null) continue;
        const path = assetPathOf(value);
        if (path !== null) element.setAttribute(attr, env.assetUrl(path));
        else if (WEB_ADDRESS.test(value)) element.removeAttribute(attr);
      }
      break;
    }
    case "a":
      element.setAttribute(
        "aria-label",
        t("doc.footnoteBackref", { ref: element.getAttribute("data-footnote-backref-idx") ?? "" }),
      );
      break;
    case "input": {
      const item = element.parentElement;
      if (item) {
        item.classList.add("task-list-item");
        item.classList.toggle("is-done", element instanceof HTMLInputElement && element.checked);
        // a checkbox is named by its task (screen readers say "checkbox, checked, Write the spec")
        const name = taskName(item);
        if (name !== "") element.setAttribute("aria-label", name);
      }
      break;
    }
    default:
      if (element.classList.contains("markdown-alert-title")) {
        const kind = alertKind(element.closest(".markdown-alert"));
        if (!element.querySelector("svg")) {
          // the renderer writes its English default when the author gave no title: only that one is translated
          const title = ALERT_TITLES[kind];
          if ((element.textContent ?? "").trim() === title.english) {
            element.textContent = t(title.key);
          }
          element.insertAdjacentHTML("afterbegin", ALERT_ICONS[kind]);
        }
      }
  }
  return result;
}

/** Enhances a freshly created block; returns the element that is the block now. */
export function enhanceBlock(block: HTMLElement, env: EnhanceEnv): HTMLElement {
  let root = block;
  const found = Array.from(block.querySelectorAll<HTMLElement>(SELECTOR));
  if (block.matches(SELECTOR)) {
    root = enhanceOne(block, env);
    // a wrapped root now holds the original: its insides are in `found` already
  }
  for (const element of found) enhanceOne(element, env);
  if (root.localName === "p" && isImageOnly(root)) root.classList.add("pm-wide");
  return root;
}
