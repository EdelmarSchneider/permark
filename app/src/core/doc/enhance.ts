// Structure added to a rendered block right after it is created, before it is shown: code and table wrappers, heading
// anchors, alert titles, task marks, asset URLs, and a quiet stand-in for pictures that live on the web. Synchronous and
// cheap; heavy things (highlighting, math, diagrams) come later, when the block is near the screen.
// Returns the block's root: a `pre` or `table` that is a block of its own is wrapped, so the wrapper is the block.

import { t } from "../i18n";

/** The kind of block, as the wrapper tells it. */
export const CODE_BLOCK = "code-block";
export const TABLE_WRAP = "table-wrap";

export type EnhanceEnv = {
  /** URL the webview loads a project file from. */
  assetUrl(path: string): string;
};

const SVG =
  'xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"';

const ALERT_ICONS: Record<string, string> = {
  note: `<svg ${SVG}><circle cx="12" cy="12" r="10"/><path d="M12 16v-4"/><path d="M12 8h.01"/></svg>`,
  tip: `<svg ${SVG}><path d="M15 14c.2-1 .7-1.7 1.5-2.5 1-.9 1.5-2.2 1.5-3.5A6 6 0 0 0 6 8c0 1 .2 2.2 1.5 3.5.7.7 1.3 1.5 1.5 2.5"/><path d="M9 18h6"/><path d="M10 22h4"/></svg>`,
  important: `<svg ${SVG}><path d="M21 15a2 2 0 0 1-2 2H7l-4 4V5a2 2 0 0 1 2-2h14a2 2 0 0 1 2 2z"/><path d="M12 7v2"/><path d="M12 13h.01"/></svg>`,
  warning: `<svg ${SVG}><path d="m21.73 18-8-14a2 2 0 0 0-3.48 0l-8 14A2 2 0 0 0 4 21h16a2 2 0 0 0 1.73-3"/><path d="M12 9v4"/><path d="M12 17h.01"/></svg>`,
  caution: `<svg ${SVG}><path d="M12 16h.01"/><path d="M12 8v4"/><path d="M15.312 2a2 2 0 0 1 1.414.586l4.688 4.688A2 2 0 0 1 22 8.688v6.624a2 2 0 0 1-.586 1.414l-4.688 4.688a2 2 0 0 1-1.414.586H8.688a2 2 0 0 1-1.414-.586l-4.688-4.688A2 2 0 0 1 2 15.312V8.688a2 2 0 0 1 .586-1.414l4.688-4.688A2 2 0 0 1 8.688 2z"/></svg>`,
};

const ALERT_TITLES = {
  note: "alert.note",
  tip: "alert.tip",
  important: "alert.important",
  warning: "alert.warning",
  caution: "alert.caution",
} as const;

type AlertKind = keyof typeof ALERT_TITLES;

function isAlertKind(kind: string): kind is AlertKind {
  return kind in ALERT_TITLES;
}

const SELECTOR =
  "pre, table, h1, h2, h3, h4, h5, h6, .markdown-alert-title, a.footnote-backref, li > input[type=checkbox], img, video, audio, source";

const PREFIX = "pm-asset:";
const REMOTE = /^(?:https?:)?\/\//i;

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

function remoteChip(source: Element, url: string): HTMLElement {
  const alt = source.getAttribute("alt") ?? "";
  const chip = document.createElement("span");
  chip.className = "remote-image";
  chip.setAttribute("role", "img");
  chip.setAttribute("data-url", url);
  chip.setAttribute("aria-label", alt);
  chip.title = t("doc.remoteImage", { url });
  chip.textContent = alt === "" ? t("doc.remoteImageNoAlt") : alt;
  return chip;
}

function enhanceOne(element: HTMLElement, env: EnhanceEnv): HTMLElement {
  let result = element;
  switch (element.localName) {
    case "pre": {
      const wrapper = wrap(element, CODE_BLOCK);
      wrapper.setAttribute("data-info", languageOf(element));
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
      } else if (REMOTE.test(src)) {
        // pictures on the web are not fetched (offline by design); a chip says what was there
        const chip = remoteChip(element, src);
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
        else if (REMOTE.test(value)) element.removeAttribute(attr);
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
      }
      break;
    }
    default:
      if (element.classList.contains("markdown-alert-title")) {
        const alert = element.closest(".markdown-alert");
        const kind = /markdown-alert-(\w+)/.exec(alert?.className ?? "")?.[1] ?? "note";
        const known = isAlertKind(kind) ? kind : "note";
        if (!element.querySelector("svg")) {
          element.textContent = t(ALERT_TITLES[known]);
          element.insertAdjacentHTML("afterbegin", ALERT_ICONS[known] ?? "");
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
