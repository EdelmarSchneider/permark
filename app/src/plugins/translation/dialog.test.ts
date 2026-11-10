// @vitest-environment jsdom
// The language dialog against the mock backend: what it lists (recently used, then all by name in the app's
// language; From: detect first), the search (name here | own name | English name | tag), the keyboard, the two
// languages it holds until confirmed, what the opener gives it (confirm label, note, what runs after), and that
// Cancel | Esc change nothing and give the keyboard back.
import "../../core/testing/media";
import { flushSync, mount, unmount } from "svelte";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createMockApi } from "../../api/mock";
import { language, tn } from "../../core/i18n";
import { LanguageDialogStore, type LanguageAsk } from "./dialog.svelte";
import { namesIn, offered } from "./languages";
import { Languages } from "./store.svelte";
import TranslationHost from "./TranslationHost.svelte";

let mock: ReturnType<typeof createMockApi>;
let languages: Languages;
let dialog: LanguageDialogStore;
let app: Record<string, unknown> | null = null;
let opener: HTMLButtonElement;

beforeEach(() => {
  mock = createMockApi();
  languages = new Languages(mock);
  dialog = new LanguageDialogStore(languages);
  opener = document.createElement("button");
  const target = document.createElement("div");
  document.body.append(opener, target);
  opener.focus();
  app = mount(TranslationHost, { target, props: { dialog } });
  flushSync();
});

afterEach(async () => {
  vi.useRealTimers();
  if (app) await unmount(app);
  app = null;
  document.body.replaceChildren();
  await language.choose("en", []);
});

/** The backend remembers `target` and `recent`; the window has read them. */
async function remembered(target: string | null, recent: string[]): Promise<void> {
  mock.probe.translation.prefs = { target, recent };
  await languages.load();
}

function open(ask: LanguageAsk = {}): void {
  dialog.ask(ask);
  flushSync();
}

const one = <E extends HTMLElement = HTMLElement>(
  selector: string,
  within: ParentNode = document,
): E => {
  const el = within.querySelector<E>(selector);
  if (el === null) throw new Error(`nothing matches ${selector}`);
  return el;
};
const box = (): HTMLElement | null => document.querySelector<HTMLElement>('[role="dialog"]');
const field = (): HTMLInputElement => one<HTMLInputElement>('[role="combobox"]');
const list = (): HTMLElement => one('[role="listbox"]');
const options = (within: ParentNode = document): HTMLElement[] =>
  Array.from(within.querySelectorAll<HTMLElement>('[role="option"]'));
const tabs = (): HTMLElement[] =>
  Array.from(document.querySelectorAll<HTMLElement>('[role="tab"]'));
const button = (action: string): HTMLButtonElement =>
  one<HTMLButtonElement>(`[data-action="${action}"]`);
const said = (el: Element | null | undefined): string =>
  (el?.textContent ?? "").replace(/\s+/g, " ").trim();
/** The name a row shows first (in the app's language). */
const nameOf = (row: Element): string => said(row.querySelector("[data-name]"));
const names = (within: ParentNode = document): string[] => options(within).map(nameOf);
const row = (name: string, within: ParentNode = document): HTMLElement => {
  const found = options(within).find((el) => nameOf(el) === name);
  if (found === undefined) throw new Error(`no row ${name}`);
  return found;
};
/** The groups of the list by their visible label. */
function groups(): Map<string, HTMLElement> {
  return new Map(
    Array.from(document.querySelectorAll<HTMLElement>('[role="group"]')).map((group) => [
      said(document.getElementById(group.getAttribute("aria-labelledby") ?? "")),
      group,
    ]),
  );
}
/** The row the keyboard is on, as the focused control names it. */
const current = (control: HTMLElement = field()): HTMLElement | null =>
  document.getElementById(control.getAttribute("aria-activedescendant") ?? "");

function type(text: string): void {
  field().value = text;
  field().dispatchEvent(new Event("input", { bubbles: true }));
  flushSync();
}

function press(el: Element, key: string, init: KeyboardEventInit = {}): KeyboardEvent {
  const event = new KeyboardEvent("keydown", { key, bubbles: true, cancelable: true, ...init });
  el.dispatchEvent(event);
  flushSync();
  return event;
}

function click(el: HTMLElement): void {
  el.click();
  flushSync();
}

describe("the language dialog", () => {
  it("is a labelled modal with two tabs, a search field that has the keyboard and the list", () => {
    expect(box()).toBeNull();
    open();
    const dialogEl = one('[role="dialog"]');
    expect(dialogEl.getAttribute("aria-modal")).toBe("true");
    expect(said(document.getElementById(dialogEl.getAttribute("aria-labelledby") ?? ""))).toBe(
      "Translate to",
    );
    expect(document.activeElement).toBe(field());
    expect(field().getAttribute("aria-controls")).toBe(list().id);
    expect(field().getAttribute("aria-label")).toBe("Search languages");
    expect(list().getAttribute("aria-label")).toBe("Languages");
    // the tabs: what is chosen on each side
    expect(one('[role="tablist"]').getAttribute("aria-label")).toBe("Languages");
    expect(tabs().map(said)).toEqual(["To: Choose…", "From: Detect automatically"]);
    expect(tabs().map((tab) => tab.getAttribute("aria-selected"))).toEqual(["true", "false"]);
    expect(tabs().map((tab) => tab.tabIndex)).toEqual([0, -1]);
    const panel = one('[role="tabpanel"]');
    expect(tabs()[0]?.getAttribute("aria-controls")).toBe(panel.id);
    expect(panel.getAttribute("aria-labelledby")).toBe(tabs()[0]?.id);
    expect(panel.contains(field()) && panel.contains(list())).toBe(true);
    // every language once, none chosen, the keyboard on the first
    expect(options()).toHaveLength(offered().length);
    expect(options().some((el) => el.getAttribute("aria-selected") === "true")).toBe(false);
    expect(current()).toBe(options()[0]);
    expect(said(one(".quality"))).toBe("Quality depends on the model you chose.");
  });

  it("lists the recently used first, then all by name in the app's language", async () => {
    await remembered("pt-BR", ["pt-BR", "de", "ja", "fr", "it", "ko", "es-ES"]);
    open();
    expect([...groups().keys()]).toEqual(["Recently used", "All languages"]);
    // five at most
    expect(names(groups().get("Recently used"))).toEqual([
      "Brazilian Portuguese",
      "German",
      "Japanese",
      "French",
      "Italian",
    ]);
    const all = names(groups().get("All languages"));
    expect(all).toEqual(offered().map(namesIn("en")).sort(new Intl.Collator("en").compare));
    // the language chosen last is the choice: named by its tab, marked in the list, where the keyboard starts
    expect(said(tabs()[0])).toBe("To: Brazilian Portuguese");
    const chosen = options().filter((el) => el.getAttribute("aria-selected") === "true");
    expect(chosen.map(nameOf)).toEqual(["Brazilian Portuguese", "Brazilian Portuguese"]);
    expect(current()).toBe(chosen[0]);
    expect(new Set(options().map((el) => el.id)).size).toBe(options().length);
  });

  it("orders the names by the rules of the app's language, not by their code points", async () => {
    await language.choose("pt-BR", []);
    open();
    const all = names();
    expect(all).toEqual(offered().map(namesIn("pt-BR")).sort(new Intl.Collator("pt-BR").compare));
    expect(all.indexOf("Árabe")).toBeGreaterThan(all.indexOf("Alemão"));
    expect(all.indexOf("Árabe")).toBeLessThan(all.indexOf("Basco"));
    expect(said(one("h2"))).toBe("Traduzir para");
  });

  it("a row: the name in the app's language, the language's own name beside it when it reads differently", () => {
    open();
    const german = row("German");
    const own = one("[lang]", german);
    expect(said(own)).toBe("Deutsch");
    expect(own.getAttribute("lang")).toBe("de");
    expect(own.getAttribute("dir")).toBe("auto");
    expect(one("[data-name]", german).getAttribute("dir")).toBe("auto");
    expect(german.getAttribute("title")).toBe("German · Deutsch");
    // English in an English window: one name
    const english = row("American English");
    expect(english.querySelector("[lang]")).toBeNull();
    expect(english.getAttribute("title")).toBe("American English");
  });

  it("typing finds a language by its own name, its English name or its tag, and marks what matched", async () => {
    await remembered("ja", ["ja"]);
    open();
    type("Deutsch");
    // only what matches, no recently used group, the keyboard on the best match
    expect([...groups().keys()]).toEqual(["All languages"]);
    expect(names()[0]).toBe("German");
    expect(names()).not.toContain("Japanese");
    expect(current()).toBe(options()[0]);
    expect(said(one(".hit", row("German")))).toBe("Deutsch");
    type("pt-br");
    expect(names()[0]).toBe("Brazilian Portuguese");
    type("germ");
    expect(said(one(".hit", row("German")))).toBe("Germ");
    type("no such language");
    expect(options()).toHaveLength(0);
    expect(said(one(".none"))).toBe("No language matches 'no such language'.");
    type("");
    expect([...groups().keys()]).toEqual(["Recently used", "All languages"]);
  });

  it("finds a language by its English name in a window that speaks another language", async () => {
    await language.choose("pt-BR", []);
    open();
    type("German");
    expect(names()[0]).toBe("Alemão");
    type("Deutsch");
    expect(names()[0]).toBe("Alemão");
  });

  it("Down + Enter choose a row and the dialog stays; the confirm button saves the choice and tells the opener", async () => {
    const then = vi.fn();
    open({ then });
    expect(button("confirm").disabled).toBe(true);
    expect(said(button("confirm"))).toBe("Save");
    expect(press(field(), "ArrowDown").defaultPrevented).toBe(true);
    const second = options()[1];
    expect(current()).toBe(second);
    expect(second?.classList.contains("active")).toBe(true);
    press(field(), "Enter");
    expect(box()).not.toBeNull();
    expect(second?.getAttribute("aria-selected")).toBe("true");
    const name = nameOf(second ?? list());
    expect(said(tabs()[0])).toBe(`To: ${name}`);
    expect(button("confirm").disabled).toBe(false);
    // nothing is chosen for the window before the confirm
    expect(languages.target).toBeNull();
    expect(then).not.toHaveBeenCalled();
    click(button("confirm"));
    expect(box()).toBeNull();
    expect(namesIn("en")(languages.target ?? "")).toBe(name);
    expect(then).toHaveBeenCalledOnce();
    // the backend remembers it
    await vi.waitFor(() => expect(languages.recent).toEqual([languages.target]));
    expect(mock.probe.translation.prefs.target).toBe(languages.target);
    expect(document.activeElement).toBe(opener);
  });

  it("Ctrl+Enter confirms from anywhere in the dialog; without a language to translate into it does nothing", () => {
    const then = vi.fn();
    open({ then });
    press(field(), "Enter", { ctrlKey: true });
    expect(box()).not.toBeNull();
    expect(then).not.toHaveBeenCalled();
    click(row("German"));
    expect(box()).not.toBeNull();
    press(field(), "Enter", { ctrlKey: true });
    expect(box()).toBeNull();
    expect(languages.target).toBe("de");
    expect(then).toHaveBeenCalledOnce();
  });

  it("Esc and Cancel change nothing and give the keyboard back to what opened the dialog", async () => {
    await remembered("de", ["de"]);
    languages.from("en-US");
    const then = vi.fn();
    open({ then });
    type("Japanese");
    press(field(), "Enter");
    click(tabs()[1] ?? list());
    click(row("Detect automatically"));
    expect(tabs().map(said)).toEqual(["To: Japanese", "From: Detect automatically"]);
    expect(press(field(), "Escape").defaultPrevented).toBe(true);
    expect(box()).toBeNull();
    expect(document.activeElement).toBe(opener);
    // a new dialog starts from what the window holds
    open({ then });
    expect(tabs().map(said)).toEqual(["To: German", "From: American English"]);
    click(row("Japanese", groups().get("All languages")));
    click(button("cancel"));
    expect(box()).toBeNull();
    expect(document.activeElement).toBe(opener);
    expect(languages).toMatchObject({ target: "de", source: "en-US", recent: ["de"] });
    expect(mock.probe.translation.prefs).toEqual({ target: "de", recent: ["de"] });
    expect(then).not.toHaveBeenCalled();
  });

  it("From: detecting comes first and is the choice until a language is named; the language translated into is off", async () => {
    await remembered("de", ["de", "ja"]);
    open();
    press(tabs()[0] ?? list(), "ArrowRight");
    expect(tabs().map((tab) => tab.getAttribute("aria-selected"))).toEqual(["false", "true"]);
    expect(document.activeElement).toBe(tabs()[1]);
    expect(one('[role="tabpanel"]').getAttribute("aria-labelledby")).toBe(tabs()[1]?.id);
    const first = options()[0];
    expect(said(first)).toBe("Detect automatically");
    expect(first?.getAttribute("aria-selected")).toBe("true");
    expect(first?.closest('[role="group"]')).toBeNull();
    expect([...groups().keys()]).toEqual(["Recently used", "All languages"]);
    // German is what the text is translated into: not a language to translate from
    const german = options().filter((el) => nameOf(el) === "German");
    expect(german).toHaveLength(2);
    expect(german.map((el) => el.getAttribute("aria-disabled"))).toEqual(["true", "true"]);
    expect(row("Japanese").hasAttribute("aria-disabled")).toBe(false);
    click(german[0] ?? list());
    expect(said(tabs()[1])).toBe("From: Detect automatically");
    type("German");
    expect(nameOf(current() ?? list())).toBe("German");
    press(field(), "Enter");
    expect(said(tabs()[1])).toBe("From: Detect automatically");
    // a named language holds for the window once confirmed
    type("american english");
    press(field(), "Enter");
    expect(said(tabs()[1])).toBe("From: American English");
    type("");
    expect(options()[0]?.getAttribute("aria-selected")).toBe("false");
    click(button("confirm"));
    expect(languages).toMatchObject({ target: "de", source: "en-US" });
  });

  it("choosing as the language to translate into the one named as source puts the source back to detecting", async () => {
    await remembered("de", ["de"]);
    languages.from("en-US");
    open();
    expect(tabs().map(said)).toEqual(["To: German", "From: American English"]);
    type("american english");
    press(field(), "Enter");
    expect(tabs().map(said)).toEqual(["To: American English", "From: Detect automatically"]);
    click(button("confirm"));
    expect(languages).toMatchObject({ target: "en-US", source: null });
  });

  it("shows the opener's confirm label and its note, which may follow the language picked", async () => {
    open({ confirm: "convert.open", note: "Creates readme.de.md next to this file" });
    expect(said(button("confirm"))).toBe("Open result");
    expect(said(one(".note"))).toBe("Creates readme.de.md next to this file");
    click(button("cancel"));
    await remembered("de", ["de"]);
    open({ note: (tag) => `Creates readme.${tag}.md next to this file` });
    expect(said(one(".note"))).toBe("Creates readme.de.md next to this file");
    click(row("Japanese", groups().get("All languages")));
    expect(said(one(".note"))).toBe("Creates readme.ja.md next to this file");
    click(button("cancel"));
    open();
    expect(document.querySelector(".note")).toBeNull();
  });

  it("a note that follows the language says nothing while none is picked", () => {
    open({ note: (tag) => `Creates readme.${tag}.md next to this file` });
    expect(document.querySelector(".note")).toBeNull();
  });

  it("the keys of the list: arrows wrap, Home | End, PageUp | PageDown; the list is a Tab stop of its own", () => {
    open();
    const all = options();
    const last = all.at(-1);
    expect(list().tabIndex).toBe(0);
    list().focus();
    expect(current(list())).toBe(all[0]);
    press(list(), "End");
    expect(current(list())).toBe(last);
    press(list(), "ArrowDown");
    expect(current(list())).toBe(all[0]);
    press(list(), "ArrowUp");
    expect(current(list())).toBe(last);
    press(list(), "Home");
    expect(current(list())).toBe(all[0]);
    press(list(), "PageDown");
    expect(current(list())).toBe(all[10]);
    press(list(), "PageUp");
    press(list(), "PageUp");
    expect(current(list())).toBe(all[0]);
    press(list(), "ArrowDown");
    press(list(), "Enter");
    expect(all[1]?.getAttribute("aria-selected")).toBe("true");
    // in the field Home | End stay with the text; the page keys move the list
    expect(press(field(), "End").defaultPrevented).toBe(false);
    expect(current()).toBe(all[1]);
    press(field(), "PageDown");
    expect(current()).toBe(all[11]);
    // never while a text is being composed
    press(field(), "ArrowDown", { isComposing: true });
    expect(current()).toBe(all[11]);
  });

  it("Tab goes through the tabs, the field, the list and the buttons, and stays in the dialog", () => {
    open();
    click(row("German"));
    const stops = [tabs()[0], field(), list(), button("cancel"), button("confirm")];
    const order = Array.from(
      one('[role="dialog"]').querySelectorAll<HTMLElement>("button, input, [tabindex]"),
    ).filter((el) => el.tabIndex >= 0);
    expect(order).toEqual(stops);
    button("confirm").focus();
    press(button("confirm"), "Tab");
    expect(document.activeElement).toBe(tabs()[0]);
    press(tabs()[0] ?? list(), "Tab", { shiftKey: true });
    expect(document.activeElement).toBe(button("confirm"));
  });

  it("says how many languages are left a moment after the list changed, not on every key", () => {
    vi.useFakeTimers();
    open();
    const live = one('[aria-live="polite"]');
    expect(said(live)).toBe("");
    type("Deu");
    type("Deutsch");
    vi.advanceTimersByTime(100);
    flushSync();
    expect(said(live)).toBe("");
    vi.advanceTimersByTime(1000);
    flushSync();
    expect(options().length).toBeGreaterThan(0);
    expect(said(live)).toBe(tn("translation.language.count", options().length));
    type("no such language");
    vi.advanceTimersByTime(1000);
    flushSync();
    expect(said(live)).toBe("0 languages");
  });

  it("one dialog at a time: a second ask while it is open is not taken", () => {
    const first = vi.fn();
    const second = vi.fn();
    open({ then: first });
    open({ then: second, confirm: "convert.open" });
    expect(document.querySelectorAll('[role="dialog"]')).toHaveLength(1);
    expect(said(button("confirm"))).toBe("Save");
    click(row("German"));
    click(button("confirm"));
    expect(first).toHaveBeenCalledOnce();
    expect(second).not.toHaveBeenCalled();
    // closed: the next ask is taken
    open({ then: second, confirm: "convert.open" });
    expect(said(button("confirm"))).toBe("Open result");
  });
});
