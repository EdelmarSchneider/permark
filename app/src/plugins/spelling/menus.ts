// The entries of the plugin's menus (drawn by the core context menu): a marked word's menu (its corrections first,
// then the person's choices about it) and the language picker (the document's language: detected or one of the
// dictionaries, each with where it stands; the default for new documents in place). A dictionary not on this
// computer is never fetched from a pick: the pick opens the download question.

import { tick } from "svelte";
import type { SpellingLanguage } from "../../api";
import type { MenuEntry } from "../../core/explorer/menu";
import { sizeText, t } from "../../core/i18n";
import { byName, languageLabel } from "./names";
import type { MenuAt, SpellingStore } from "./store.svelte";

const SEPARATOR: MenuEntry = { kind: "separator" };

/** Runs `then` once the menu that ran it has closed and given the keyboard back (what opens next returns it there). */
function later(then: () => void): void {
  void tick().then(then);
}

function item(label: string, run: () => void, detail: string | null = null): MenuEntry {
  return { kind: "item", label, run, ...(detail === null || detail === "" ? {} : { detail }) };
}

/** The menu of the marked word `at`: its corrections (`suggestions`), Add to dictionary, Ignore, the language. */
export function wordEntries(
  store: SpellingStore,
  at: MenuAt,
  suggestions: readonly string[],
): MenuEntry[] {
  const fixes: MenuEntry[] =
    suggestions.length === 0
      ? [{ kind: "item", label: t("spelling.menu.none"), disabled: true, run: () => {} }]
      : suggestions.map((text) => item(text, () => void store.replace(at.index, text)));
  const lang = store.document?.language.langs[0];
  return [
    ...fixes,
    SEPARATOR,
    item(t("spelling.menu.add"), () => void store.addToDictionary(at.word)),
    item(t("spelling.menu.ignore"), () => store.ignore(at.word)),
    SEPARATOR,
    {
      kind: "item",
      label: t("spelling.menu.language", {
        language: lang === undefined ? "" : languageLabel(lang),
      }),
      run: () => later(() => store.openPicker(at.x, at.y)),
    },
  ];
}

/** Where a dictionary stands, in words (`null`: ready, nothing to say). */
export function stateText(language: SpellingLanguage): string | null {
  const { state } = language;
  switch (state.kind) {
    case "ready":
      return null;
    case "absent":
      return t("spelling.languages.absent", { size: sizeText(language.bytes) });
    case "downloading":
      return t("spelling.languages.downloading", {
        percent: state.total > 0 ? Math.floor((state.done / state.total) * 100) : 0,
      });
    case "failed":
      return t("spelling.languages.failed");
    default:
      return state satisfies never;
  }
}

/** A language picked for the document: declared when its dictionary is here or on its way, else asked about. */
function pick(store: SpellingStore, language: SpellingLanguage): void {
  switch (language.state.kind) {
    case "ready":
    case "downloading":
      void store.declare(language.id);
      return;
    case "absent":
    case "failed":
      later(() => store.askDownload(language.id, true));
      return;
    default:
      language.state satisfies never;
  }
}

/** The language picker's list at `at`: the download the document needs first, Detect automatically, every
 *  dictionary, then the default for new documents (opens its own list in place). */
export function languageEntries(store: SpellingStore, at: { x: number; y: number }): MenuEntry[] {
  const doc = store.document?.language ?? null;
  const current = doc?.langs[0] ?? null;
  const declared = doc?.source === "declared" ? current : null;
  const out: MenuEntry[] = [];
  const [missing] = store.needsDownload;
  const needed = missing === undefined ? undefined : store.languageOf(missing);
  if (needed !== undefined && needed.state.kind !== "downloading") {
    out.push(
      item(
        t("spelling.languages.offer", { language: languageLabel(needed.id) }),
        () => later(() => store.askDownload(needed.id, false)),
        sizeText(needed.bytes),
      ),
      SEPARATOR,
    );
  }
  out.push(
    item(
      t("spelling.languages.detect"),
      () => void store.declare(null),
      declared === null && current !== null
        ? t("spelling.languages.now", { language: languageLabel(current) })
        : null,
    ),
    SEPARATOR,
  );
  for (const id of byName(store.languages.map((l) => l.id))) {
    const language = store.languageOf(id);
    if (language === undefined) continue;
    const details = [id === declared ? t("spelling.languages.inUse") : null, stateText(language)];
    out.push(
      item(
        languageLabel(id),
        () => pick(store, language),
        details.filter((d) => d !== null).join(" · "),
      ),
    );
  }
  const fallback = store.defaultLanguage?.lang;
  out.push(SEPARATOR, {
    kind: "item",
    label: t("spelling.languages.default", {
      language: fallback === undefined ? "…" : languageLabel(fallback),
    }),
    opens: true,
    run: () => store.openPicker(at.x, at.y, "default"),
  });
  return out;
}

/** The default for new documents, in place of the picker's list: every dictionary language. */
export function defaultEntries(store: SpellingStore): MenuEntry[] {
  const chosen = store.defaultLanguage?.lang;
  return byName(store.languages.map((l) => l.id)).map((id) =>
    item(
      languageLabel(id),
      () => void store.setDefault(id),
      id === chosen ? t("spelling.languages.inUse") : null,
    ),
  );
}
