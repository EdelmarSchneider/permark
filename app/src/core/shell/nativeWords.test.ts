// The words of the app's own system dialogs (the backend holds no translations): in every language they are fit for
// a system dialog, as the backend checks them (an unfit set is ignored and the dialog stays in English).
import { afterAll, describe, expect, it } from "vitest";
import type { NativeWords } from "../../api";
import { LOCALES, language, t } from "../i18n";
import { nativeWords } from "./nativeWords";

/** Longest word the backend takes (a system dialog has no scroll). */
const MAX_WORDS = 600;

/** Every word the backend can show (a field left out stays English there). */
function expected(): Required<NativeWords> {
  return {
    notResponding: t("stuck.title"),
    notRespondingDetail: t("stuck.detail"),
    closeAnyway: t("stuck.closeAnyway"),
    wait: t("stuck.wait"),
    engineStopped: t("stuck.engineStopped"),
    windowFailed: t("stuck.windowFailed"),
    viewFailedExit: t("stuck.viewFailedExit"),
    language: language.locale,
  };
}

afterAll(async () => {
  await language.choose("en", []);
});

describe("the words of the system dialogs", () => {
  it("come from the interface language, every one of them", async () => {
    await language.choose("de", []);
    expect(nativeWords()).toEqual(expected());
    expect(nativeWords().closeAnyway).toBe("Trotzdem schließen");
    expect(nativeWords().language).toBe("de");
  });

  it("the not-answering text names a page of the window: Close anyway may drop unsaved changes in the window's tabs", async () => {
    await language.choose("en", []);
    expect(nativeWords().notRespondingDetail).toBe(
      "A page in this window isn't answering. If you close anyway, unsaved changes in this window's tabs may be lost.",
    );
  });

  it.each(LOCALES)(
    "%s: every word sent, none empty, none too long, no control character, two different buttons",
    async (locale) => {
      await language.choose(locale, []);
      const words = nativeWords();
      expect(Object.keys(words).sort()).toEqual(Object.keys(expected()).sort());
      for (const word of Object.values(words)) {
        expect(word.trim()).not.toBe("");
        expect([...word].length).toBeLessThanOrEqual(MAX_WORDS);
        expect([...word].filter((c) => c !== "\n" && /\p{Cc}/u.test(c))).toEqual([]);
      }
      expect(words.closeAnyway.trim()).not.toBe(words.wait.trim());
    },
  );
});
