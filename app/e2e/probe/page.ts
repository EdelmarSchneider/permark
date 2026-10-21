// In-page collectors (spec: ux-probe.md §7). Each exported function is serialized into the page by Playwright: it MUST be
// self-contained (no imports, no outer names at run time; types only).

export type Box = { x: number; y: number; w: number; h: number };

export type FocusFact =
  | { kind: "body" }
  | {
      kind: "element";
      selector: string;
      role: string;
      name: string;
      tag: string;
      box: Box;
      visible: boolean;
      inViewport: boolean;
      clipped: boolean;
      /** Share of 5 sample points (center + corners) under a foreign element. */
      covered: number;
      coveredBy: string | null;
      focusVisible: boolean;
      indicator: boolean;
      current: boolean | null;
      group: { selector: string; current: string | null } | null;
    };

export type RegionFact = { selector: string; label: string; box: Box; overlay: boolean };
export type PaintsOver = { element: string; from: string; over: string; box: Box };
export type CoveredFact = { element: string; name: string; by: string };
export type ScrollerFact = {
  selector: string;
  box: Box;
  /** Width of the scroll bar's lane in px (0: overlay scroll bars). */
  gutter: number;
  /** The lane on screen (viewport px), for the pixel check. */
  lane: Box | null;
};

export type Snapshot = {
  focus: FocusFact;
  regions: RegionFact[];
  paintsOver: PaintsOver[];
  covered: CoveredFact[];
  scrollers: ScrollerFact[];
  /** Lines of the watched region's text. */
  text: string[];
  /** Text of alerts, status + live regions, tooltips, dialogs. */
  live: string[];
  /** Visible `title` | `aria-keyshortcuts` texts naming keys: "(Esc)". */
  keyHints: { element: string; hint: string }[];
  viewport: { width: number; height: number };
  theme: { theme: string | null; tone: string | null };
  /** The element was found for `watch`. */
  watched: boolean;
};

export type Observed = {
  /** Net changes: an attribute | text back to its old value, a node added then removed, do not count. */
  mutations: number;
  firstAtMs: number | null;
  /** Net changes outside the action target and its group. */
  outside: number;
  /** Last raw mutation (net or not): for waiting until the page is quiet. */
  lastAtMs: number | null;
  /** Focus is now elsewhere than before the action and than the action's target. */
  focusMoved: boolean;
  /** The element focused before the action is still in the page. */
  previousFocusAlive: boolean;
  /** Where the page changed: elements (outermost first seen), max 10; `@name` = attributes that changed. */
  changed: string[];
};

/** Everything the probe reads from the page at one moment. */
export function collect(arg: { watch: string | null }): Snapshot {
  const vw = window.innerWidth;
  const vh = window.innerHeight;
  const boxOf = (r: DOMRect): Box => ({
    x: Math.round(r.x),
    y: Math.round(r.y),
    w: Math.round(r.width),
    h: Math.round(r.height),
  });
  const short = (s: string, n = 80): string => {
    const t = s.replace(/\s+/g, " ").trim();
    return t.length > n ? `${t.slice(0, n - 1)}…` : t;
  };
  const visible = (el: Element): boolean => {
    if (el.closest("[inert]")) return false;
    const r = el.getBoundingClientRect();
    if (r.width < 1 || r.height < 1) return false;
    const check = (el as Element & { checkVisibility?: (o: object) => boolean }).checkVisibility;
    if (check) return check.call(el, { opacityProperty: true, visibilityProperty: true });
    const s = getComputedStyle(el);
    return s.visibility !== "hidden" && s.display !== "none" && s.opacity !== "0";
  };
  const cssPath = (el: Element): string => {
    const part = (e: Element): string => {
      const tag = e.tagName.toLowerCase();
      if (e.id) return `${tag}#${e.id}`;
      for (const a of ["data-path", "data-region", "data-id", "aria-label", "role"]) {
        const v = e.getAttribute(a);
        if (v !== null && v !== "") return `${tag}[${a}="${short(v, 40)}"]`;
      }
      const classes = [...e.classList].filter((c) => !c.startsWith("svelte-")).slice(0, 2);
      return classes.length > 0 ? `${tag}.${classes.join(".")}` : tag;
    };
    const parts: string[] = [];
    let at: Element | null = el;
    while (at && at !== document.body && parts.length < 4) {
      parts.unshift(part(at));
      if (at.id) break;
      at = at.parentElement;
    }
    return parts.join(" > ") || el.tagName.toLowerCase();
  };
  const IMPLICIT: Record<string, string> = {
    button: "button",
    nav: "navigation",
    main: "main",
    aside: "complementary",
    header: "banner",
    footer: "contentinfo",
    ul: "list",
    ol: "list",
    li: "listitem",
    textarea: "textbox",
    select: "combobox",
    dialog: "dialog",
    h1: "heading",
    h2: "heading",
    h3: "heading",
    h4: "heading",
  };
  const roleOf = (el: Element): string => {
    const explicit = el.getAttribute("role");
    if (explicit) return explicit;
    const tag = el.tagName.toLowerCase();
    if (tag === "a") return el.hasAttribute("href") ? "link" : "generic";
    if (tag === "input") {
      const type = (el as HTMLInputElement).type;
      return type === "checkbox" || type === "radio"
        ? type
        : type === "button"
          ? "button"
          : "textbox";
    }
    if (tag === "section") return el.hasAttribute("aria-label") ? "region" : "generic";
    return IMPLICIT[tag] ?? "generic";
  };
  const nameOf = (el: Element): string => {
    const by = el.getAttribute("aria-labelledby");
    if (by) {
      const t = by
        .split(/\s+/)
        .map((id) => document.getElementById(id)?.textContent ?? "")
        .join(" ");
      if (t.trim()) return short(t);
    }
    const label = el.getAttribute("aria-label");
    if (label) return short(label);
    if (el instanceof HTMLInputElement || el instanceof HTMLTextAreaElement) {
      const l = el.labels?.[0]?.textContent;
      if (l) return short(l);
      if (el.placeholder) return short(el.placeholder);
    }
    const alt = el.getAttribute("alt");
    if (alt) return short(alt);
    const txt = (el as HTMLElement).innerText ?? el.textContent ?? "";
    if (txt.trim()) return short(txt);
    return short(el.getAttribute("title") ?? "");
  };
  const CURRENT =
    '[aria-current]:not([aria-current="false"]), [aria-selected="true"], [aria-checked="true"]';
  const GROUP =
    '[role="tablist"], [role="radiogroup"], [role="listbox"], [role="menu"], [role="menubar"], [role="tree"], [role="grid"], nav, ul, ol';
  const groupOf = (el: Element): Element | null => {
    let at = el.parentElement?.closest(GROUP) ?? null;
    while (at) {
      if (at.querySelector(CURRENT)) return at;
      at = at.parentElement?.closest(GROUP) ?? null;
    }
    return null;
  };
  const isCurrent = (el: Element, group: Element): boolean => {
    for (let at: Element | null = el; at && at !== group; at = at.parentElement) {
      if (at.matches(CURRENT)) return true;
    }
    return false;
  };
  /** Sample points of a box (center + 4 corners 2 px in), cut to the viewport. */
  const points = (r: DOMRect): [number, number][] => {
    const left = Math.max(r.left, 0) + 2;
    const right = Math.min(r.right, vw) - 2;
    const top = Math.max(r.top, 0) + 2;
    const bottom = Math.min(r.bottom, vh) - 2;
    if (right <= left || bottom <= top) return [];
    return [
      [(left + right) / 2, (top + bottom) / 2],
      [left, top],
      [right, top],
      [left, bottom],
      [right, bottom],
    ];
  };
  const related = (a: Element, b: Element): boolean => a === b || a.contains(b) || b.contains(a);
  const clear = (color: string): boolean =>
    color === "transparent" || /rgba\([^)]*,\s*0\)$/.test(color);
  /** The element draws something itself: background, border, shadow, its own text, a picture. */
  const paints = (el: Element): boolean => {
    const s = getComputedStyle(el);
    if (s.visibility === "hidden" || s.opacity === "0") return false;
    if (!clear(s.backgroundColor) || s.backgroundImage !== "none" || s.boxShadow !== "none")
      return true;
    for (const side of ["Top", "Right", "Bottom", "Left"] as const) {
      const width = parseFloat(s.getPropertyValue(`border-${side.toLowerCase()}-width`));
      if (width > 0 && !clear(s.getPropertyValue(`border-${side.toLowerCase()}-color`)))
        return true;
    }
    if (/^(img|svg|video|canvas|input|textarea|select|iframe)$/i.test(el.tagName)) return true;
    return [...el.childNodes].some(
      (n) => n.nodeType === Node.TEXT_NODE && (n.textContent ?? "").trim() !== "",
    );
  };
  /** `hit` (at a point of `target`) hides it: hit, or one of its ancestors that is not around `target`, draws there. */
  const hides = (hit: Element, target: Element): boolean => {
    for (let at: Element | null = hit; at && !at.contains(target); at = at.parentElement) {
      if (paints(at)) return true;
    }
    return false;
  };
  const OVERLAY =
    '[role="dialog"], [role="alertdialog"], [role="menu"], [role="listbox"], [role="tooltip"], dialog, [popover]';

  // ---- focus ----
  let focus: FocusFact = { kind: "body" };
  const active = document.activeElement;
  if (active && active !== document.body && active !== document.documentElement) {
    const r = active.getBoundingClientRect();
    const pts = points(r);
    let hidden = 0;
    let coveredBy: string | null = null;
    for (const [x, y] of pts) {
      const hit = document.elementFromPoint(x, y);
      if (hit && !related(hit, active) && hides(hit, active)) {
        hidden += 1;
        coveredBy ??= cssPath(hit);
      }
    }
    let clipped = pts.length === 0;
    for (
      let at = active.parentElement;
      at && at !== document.documentElement;
      at = at.parentElement
    ) {
      const s = getComputedStyle(at);
      if (s.overflowX === "visible" && s.overflowY === "visible") continue;
      const c = at.getBoundingClientRect();
      if (
        r.left < c.left - 1 ||
        r.right > c.right + 1 ||
        r.top < c.top - 1 ||
        r.bottom > c.bottom + 1
      ) {
        clipped = true;
        break;
      }
    }
    const s = getComputedStyle(active);
    const outline = s.outlineStyle !== "none" && parseFloat(s.outlineWidth) > 0;
    const shadow = s.boxShadow !== "none" && s.boxShadow !== "";
    const group = groupOf(active);
    const currentEl = group?.querySelector(CURRENT) ?? null;
    let focusVisible = false;
    try {
      focusVisible = active.matches(":focus-visible");
    } catch {
      focusVisible = false;
    }
    focus = {
      kind: "element",
      selector: cssPath(active),
      role: roleOf(active),
      name: nameOf(active),
      tag: active.tagName.toLowerCase(),
      box: boxOf(r),
      visible: visible(active),
      inViewport: r.right > 0 && r.bottom > 0 && r.left < vw && r.top < vh,
      clipped,
      covered: pts.length === 0 ? 1 : hidden / pts.length,
      coveredBy,
      focusVisible,
      indicator: outline || shadow,
      current: group ? isCurrent(active, group) : null,
      group: group
        ? { selector: cssPath(group), current: currentEl ? nameOf(currentEl) : null }
        : null,
    };
  }

  // ---- regions: landmarks + fixed | sticky bars; top-level = no region around them ----
  const REGION =
    'header, nav, main, aside, footer, section[aria-label], section[aria-labelledby], [role="region"], [role="banner"], [role="navigation"], [role="main"], [role="complementary"], [role="contentinfo"], [role="toolbar"], [role="dialog"], [role="alertdialog"], [role="search"], [data-region], dialog';
  const candidates = new Set<Element>([...document.querySelectorAll(REGION)].filter(visible));
  for (const el of document.querySelectorAll("body *")) {
    if (candidates.has(el)) continue;
    const pos = getComputedStyle(el).position;
    if ((pos === "fixed" || pos === "sticky") && visible(el)) candidates.add(el);
  }
  const top = [...candidates].filter((el) => {
    for (let at = el.parentElement; at; at = at.parentElement) if (candidates.has(at)) return false;
    return true;
  });
  const regions: RegionFact[] = top.map((el) => ({
    selector: cssPath(el),
    label: nameOf(el).slice(0, 40),
    box: boxOf(el.getBoundingClientRect()),
    overlay: el.matches(OVERLAY) || getComputedStyle(el).position === "fixed",
  }));

  // ---- content of one area painted over another ----
  const paintsOver: PaintsOver[] = [];
  const reported: Element[] = [];
  for (const area of top) {
    if (area.matches(OVERLAY)) continue;
    const ar = area.getBoundingClientRect();
    const others = top.filter((o) => o !== area && !o.matches(OVERLAY));
    for (const el of area.querySelectorAll("*")) {
      if (paintsOver.length >= 20) break;
      const r = el.getBoundingClientRect();
      if (r.width < 1 || r.height < 1) continue;
      if (
        r.left >= ar.left - 1 &&
        r.right <= ar.right + 1 &&
        r.top >= ar.top - 1 &&
        r.bottom <= ar.bottom + 1
      ) {
        continue;
      }
      for (const other of others) {
        const or = other.getBoundingClientRect();
        const x1 = Math.max(r.left, or.left);
        const x2 = Math.min(r.right, or.right);
        const y1 = Math.max(r.top, or.top);
        const y2 = Math.min(r.bottom, or.bottom);
        if (x2 - x1 <= 1 || y2 - y1 <= 1) continue;
        const hit = document.elementFromPoint((x1 + x2) / 2, (y1 + y2) / 2);
        if (!hit || !area.contains(hit) || other.contains(hit)) continue;
        // only the outermost overflowing element is reported, not each of its children
        if (reported.some((done) => done.contains(el))) continue;
        reported.push(el);
        paintsOver.push({
          element: cssPath(el),
          from: cssPath(area),
          over: cssPath(other),
          box: {
            x: Math.round(x1),
            y: Math.round(y1),
            w: Math.round(x2 - x1),
            h: Math.round(y2 - y1),
          },
        });
      }
    }
  }

  // ---- controls under a foreign element ----
  const covered: CoveredFact[] = [];
  const CONTROLS =
    'button, a[href], input, select, textarea, summary, [role="button"], [role="tab"], [role="treeitem"], [role="menuitem"], [role="radio"], [role="checkbox"], [role="option"], [tabindex]:not([tabindex="-1"])';
  for (const el of document.querySelectorAll(CONTROLS)) {
    if (covered.length >= 20) break;
    if (!visible(el)) continue;
    const r = el.getBoundingClientRect();
    if (r.right <= 0 || r.bottom <= 0 || r.left >= vw || r.top >= vh) continue;
    const [center] = points(r);
    if (!center) continue;
    const hit = document.elementFromPoint(center[0], center[1]);
    if (!hit || related(hit, el) || hit.closest(OVERLAY) || !hides(hit, el)) continue;
    // a region cut by its scroll container is not covered, it is scrolled away
    let scrolledAway = false;
    for (let at = el.parentElement; at; at = at.parentElement) {
      const s = getComputedStyle(at);
      if (s.overflowY === "visible" && s.overflowX === "visible") continue;
      const c = at.getBoundingClientRect();
      if (center[1] < c.top || center[1] > c.bottom || center[0] < c.left || center[0] > c.right) {
        scrolledAway = true;
        break;
      }
    }
    if (scrolledAway) continue;
    covered.push({ element: cssPath(el), name: nameOf(el), by: cssPath(hit) });
  }

  // ---- scrolling elements ----
  const scrollers: ScrollerFact[] = [];
  const scrollCandidates = [document.scrollingElement, ...document.querySelectorAll("body *")];
  for (const el of scrollCandidates) {
    if (!el || scrollers.length >= 12) continue;
    if (!(el instanceof HTMLElement)) continue;
    if (el.scrollHeight <= el.clientHeight + 1) continue;
    const s = getComputedStyle(el);
    const root = el === document.scrollingElement;
    if (!root && s.overflowY !== "auto" && s.overflowY !== "scroll") continue;
    if (!root && !visible(el)) continue;
    const r = root ? new DOMRect(0, 0, vw, vh) : el.getBoundingClientRect();
    const bl = root ? 0 : parseFloat(s.borderLeftWidth) || 0;
    const br = root ? 0 : parseFloat(s.borderRightWidth) || 0;
    const bt = root ? 0 : parseFloat(s.borderTopWidth) || 0;
    const gutter = Math.max(0, Math.round((root ? vw : el.offsetWidth) - el.clientWidth - bl - br));
    const lane =
      gutter > 0
        ? {
            x: Math.round(r.right - br - gutter),
            y: Math.round(r.top + bt),
            w: gutter,
            h: Math.round(el.clientHeight),
          }
        : null;
    scrollers.push({ selector: root ? "html" : cssPath(el), box: boxOf(r), gutter, lane });
  }

  // ---- text of the watched region; live text; key hints ----
  const watchEl = arg.watch === null ? document.body : document.querySelector(arg.watch);
  const text = watchEl
    ? ((watchEl as HTMLElement).innerText ?? "")
        .split("\n")
        .map((l) => l.trim())
        .filter((l) => l !== "")
        .slice(0, 800)
    : [];
  const live = [
    ...document.querySelectorAll(
      '[role="alert"], [role="status"], [aria-live]:not([aria-live="off"]), [role="tooltip"], [role="dialog"], [role="alertdialog"], dialog[open]',
    ),
  ]
    .map((el) => short((el as HTMLElement).innerText ?? el.textContent ?? "", 200))
    .filter((t) => t !== "");
  const keyHints: Snapshot["keyHints"] = [];
  for (const el of document.querySelectorAll("[title], [aria-keyshortcuts]")) {
    if (keyHints.length >= 60 || !visible(el)) continue;
    const shortcuts = el.getAttribute("aria-keyshortcuts");
    if (shortcuts) keyHints.push({ element: cssPath(el), hint: shortcuts });
    const title = el.getAttribute("title") ?? "";
    for (const m of title.matchAll(/\(([^()]{1,24})\)/g)) {
      if (m[1]) keyHints.push({ element: cssPath(el), hint: m[1] });
    }
  }

  const html = document.documentElement;
  return {
    focus,
    regions,
    paintsOver,
    covered,
    scrollers,
    text,
    live,
    keyHints,
    viewport: { width: vw, height: vh },
    theme: { theme: html.getAttribute("data-theme"), tone: html.getAttribute("data-tone") },
    watched: watchEl !== null,
  };
}

type Recording = {
  mo: MutationObserver;
  t0: number;
  self: Element | null;
  target: Element | null;
  before: Element | null;
  /** Raw records in order: what changed, its old value, when. */
  log: {
    kind: "attr" | "text" | "add" | "remove";
    node: Node;
    name: string | null;
    old: string | null;
    at: number;
  }[];
  last: number | null;
};

/** Starts recording DOM mutations; `target` (the acted-on element) + its radio | tab | list group are "self". */
export function observeStart(acted: Element | null): void {
  const GROUP =
    '[role="radiogroup"], [role="tablist"], [role="listbox"], [role="menu"], [role="menubar"]';
  // a key pressed with nothing focused acts on the page: no "self" part
  const target = acted === document.body || acted === document.documentElement ? null : acted;
  const rec = {
    t0: performance.now(),
    self: target ? (target.closest(GROUP) ?? target) : null,
    target,
    before: document.activeElement,
    log: [] as Recording["log"],
    last: null as number | null,
  } as Omit<Recording, "mo"> & { mo?: MutationObserver };
  const mo = new MutationObserver((records) => {
    const at = performance.now() - rec.t0;
    rec.last = at;
    for (const r of records) {
      if (rec.log.length > 20_000) return;
      if (r.type === "attributes") {
        rec.log.push({ kind: "attr", node: r.target, name: r.attributeName, old: r.oldValue, at });
      } else if (r.type === "characterData") {
        rec.log.push({ kind: "text", node: r.target, name: null, old: r.oldValue, at });
      } else {
        for (const n of r.addedNodes)
          rec.log.push({ kind: "add", node: n, name: null, old: null, at });
        for (const n of r.removedNodes)
          rec.log.push({ kind: "remove", node: n, name: null, old: null, at });
      }
    }
  });
  rec.mo = mo;
  mo.observe(document.body, {
    subtree: true,
    childList: true,
    attributes: true,
    characterData: true,
    attributeOldValue: true,
    characterDataOldValue: true,
  });
  (window as unknown as { __uxProbe: unknown }).__uxProbe = rec;
}

/** What was recorded so far, as net changes (does not stop). */
export function observeRead(): Observed {
  const rec = (window as unknown as { __uxProbe?: Recording }).__uxProbe;
  if (!rec) {
    return {
      mutations: 0,
      firstAtMs: null,
      outside: 0,
      lastAtMs: null,
      focusMoved: false,
      previousFocusAlive: true,
      changed: [],
    };
  }
  // first old value per (node, attribute) | text node; nodes added | removed
  const firstOld = new Map<Node, Map<string, string | null>>();
  const net: { node: Node; name: string | null; at: number }[] = [];
  const seen = new Set<string>();
  const ids = new Map<Node, number>();
  const idOf = (n: Node): number => {
    let id = ids.get(n);
    if (id === undefined) {
      id = ids.size;
      ids.set(n, id);
    }
    return id;
  };
  for (const e of rec.log) {
    if (e.kind === "attr" || e.kind === "text") {
      const key = e.name ?? "#text";
      const olds = firstOld.get(e.node) ?? new Map<string, string | null>();
      if (!olds.has(key)) olds.set(key, e.old);
      firstOld.set(e.node, olds);
    }
  }
  for (const e of [...rec.log].reverse()) {
    const key = `${e.kind}:${idOf(e.node)}:${e.name ?? ""}`;
    if (seen.has(key)) continue;
    seen.add(key);
    if (e.kind === "attr" && e.node instanceof Element && e.name !== null) {
      const old = firstOld.get(e.node)?.get(e.name) ?? null;
      if (e.node.isConnected && e.node.getAttribute(e.name) !== old) net.push(e);
    } else if (e.kind === "text") {
      const old = firstOld.get(e.node)?.get("#text") ?? null;
      if (e.node.isConnected && e.node.textContent !== old) net.push(e);
    } else if (e.kind === "add") {
      if (
        e.node.isConnected &&
        !(e.node instanceof Text && (e.node.textContent ?? "").trim() === "")
      )
        net.push(e);
    } else if (e.kind === "remove") {
      if (!e.node.isConnected) net.push(e);
    }
  }
  net.reverse();
  const elementOf = (n: Node): Element | null => (n instanceof Element ? n : n.parentElement);
  let outside = 0;
  const changed: Element[] = [];
  const attributes = new Set<string>();
  for (const e of net) {
    // a removed node is out of the page: its old place is unknown, it counts as outside
    const el = e.node.isConnected ? elementOf(e.node) : null;
    if (!rec.self || !el || !rec.self.contains(el)) outside += 1;
    if (e.name !== null) attributes.add(e.name);
    if (el && changed.length < 10 && !changed.some((c) => c.contains(el))) changed.push(el);
  }
  const now = document.activeElement;
  const onTarget = rec.target !== null && now !== null && rec.target.contains(now);
  const path = (el: Element): string => {
    const parts: string[] = [];
    for (
      let at: Element | null = el;
      at && at !== document.body && parts.length < 3;
      at = at.parentElement
    ) {
      const label = at.getAttribute("aria-label") ?? at.getAttribute("data-path");
      const cls = [...at.classList].filter((c) => !c.startsWith("svelte-"))[0];
      parts.unshift(
        at.tagName.toLowerCase() + (label ? `[${label.slice(0, 30)}]` : cls ? `.${cls}` : ""),
      );
    }
    return parts.join(" > ");
  };
  return {
    mutations: net.length,
    firstAtMs: net[0] ? Math.round(net[0].at) : null,
    outside,
    lastAtMs: rec.last === null ? null : Math.round(rec.last),
    focusMoved: now !== rec.before && !onTarget,
    previousFocusAlive: rec.before === null || rec.before.isConnected,
    changed: changed.map(path).concat([...attributes].slice(0, 10).map((a) => `@${a}`)),
  };
}

/** Stops recording. */
export function observeStop(): void {
  const w = window as unknown as { __uxProbe?: Recording };
  w.__uxProbe?.mo.disconnect();
  delete w.__uxProbe;
}
