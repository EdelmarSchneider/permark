// Plain words of the Local AI page: jobs, what a model can do, languages, fit hints, sizes, test results, refusals. No
// jargon: the person reads "Search by meaning", "Runs slowly here", "3.4 GB".

import type { ApiError } from "../../api";
import type { DownloadError } from "../../api/generated/DownloadError";
import type { ModelFit } from "../../api/generated/ModelFit";
import { apiErrorText } from "../../core/errors";
import { language, sizeText, t, tn, type MessageKey } from "../../core/i18n";
import type { AiTask, ModelCapability } from "./tasks";

/** Languages named before "and N more". */
const SHOWN_LANGUAGES = 4;

export function taskKey(task: AiTask): MessageKey {
  switch (task) {
    case "correction":
      return "localAi.task.correction";
    case "translation":
      return "localAi.task.translation";
    case "semanticIndex":
      return "localAi.task.semanticIndex";
    case "readAloud":
      return "localAi.task.readAloud";
    case "dictation":
      return "localAi.task.dictation";
    case "assistant":
      return "localAi.task.assistant";
    default:
      return task satisfies never;
  }
}

/** What the job does for the person (row hint). */
export function taskHintKey(task: AiTask): MessageKey {
  switch (task) {
    case "correction":
      return "localAi.task.correction.hint";
    case "translation":
      return "localAi.task.translation.hint";
    case "semanticIndex":
      return "localAi.task.semanticIndex.hint";
    case "readAloud":
      return "localAi.task.readAloud.hint";
    case "dictation":
      return "localAi.task.dictation.hint";
    case "assistant":
      return "localAi.task.assistant.hint";
    default:
      return task satisfies never;
  }
}

export function capabilityKey(capability: ModelCapability): MessageKey {
  switch (capability) {
    case "embed":
      return "localAi.cap.embed";
    case "chat":
      return "localAi.cap.chat";
    case "tools":
      return "localAi.cap.tools";
    case "translate":
      return "localAi.cap.translate";
    case "transcribe":
      return "localAi.cap.transcribe";
    case "speak":
      return "localAi.cap.speak";
    case "vision":
      return "localAi.cap.vision";
    default:
      return capability satisfies never;
  }
}

export function fitKey(fit: ModelFit): MessageKey {
  switch (fit) {
    case "runsWell":
      return "localAi.fit.runsWell";
    case "slowCpuOnly":
      return "localAi.fit.slowCpuOnly";
    case "notRecommended":
      return "localAi.fit.notRecommended";
    case "unknown":
      return "localAi.fit.unknown";
    default:
      return fit satisfies never;
  }
}

/** Name of one BCP-47 language in the current language (`mul` = many; unknown code -> the code). */
function languageName(code: string, names: Intl.DisplayNames): string {
  if (code === "mul") return t("localAi.languages.many");
  try {
    return names.of(code) ?? code;
  } catch {
    return code;
  }
}

/** "German, Japanese and Korean"; past four: "English, Arabic, Chinese, French, and 9 more" (list style of the
 *  current language). */
export function languagesText(codes: readonly string[]): string {
  const locale = language.locale;
  const names = new Intl.DisplayNames(locale, { type: "language" });
  const shown = codes.slice(0, SHOWN_LANGUAGES).map((code) => languageName(code, names));
  const rest = codes.length - shown.length;
  const parts = rest > 0 ? [...shown, tn("localAi.languages.more", rest)] : shown;
  return new Intl.ListFormat(locale, { type: "conjunction" }).format(parts);
}

/** A memory size: "6 GB", "128 MB" (binary units with the familiar names, as systems show memory: 16 GiB -> 16 GB). */
export function memoryText(bytes: number): string {
  const giga = bytes >= 2 ** 30;
  return new Intl.NumberFormat(language.locale, {
    style: "unit",
    unit: giga ? "gigabyte" : "megabyte",
    maximumFractionDigits: giga ? 1 : 0,
  }).format(bytes / (giga ? 2 ** 30 : 2 ** 20));
}

/** A share of a download: "42%" (rounded down: 100% only when it is all there). */
export function percentText(done: number, total: number): string {
  const share = total > 0 ? Math.floor((Math.min(done, total) / total) * 100) / 100 : 0;
  return new Intl.NumberFormat(language.locale, { style: "percent" }).format(share);
}

/** Why a download stopped, in words. */
export function downloadErrorText(error: DownloadError): string {
  switch (error.kind) {
    case "offline":
      return t("localAi.dl.failed.offline");
    case "http":
      return t("localAi.dl.failed.http", { status: error.status });
    case "hostNotAllowed":
      return t("localAi.dl.failed.hostNotAllowed");
    case "integrity":
      return t("localAi.dl.failed.integrity");
    case "noSpace":
      return t("localAi.dl.failed.noSpace", {
        needed: sizeText(error.needed),
        free: sizeText(error.free),
      });
    case "disk":
      return t("localAi.dl.failed.disk");
    case "cancelled":
      return t("localAi.dl.failed.cancelled");
    case "busy":
      return t("localAi.dl.failed.busy");
    default:
      return error satisfies never;
  }
}

/** A refusal's reason (backend detail | subject) -> its own words; anything else -> the general error text. */
function reasonKey(reason: string | null): MessageKey | null {
  switch (reason) {
    case "incompatible":
      return "localAi.err.incompatible";
    case "notInstalled":
      return "localAi.err.notInstalled";
    case "downloading":
      return "localAi.err.downloading";
    case "moving":
      return "localAi.err.moving";
    case "store":
      return "localAi.err.store";
    case "license":
      return "localAi.err.license";
    case "token":
      return "localAi.err.token";
    case "withdrawn":
      return "localAi.err.withdrawn";
    case "noUpdate":
      return "localAi.err.noUpdate";
    case "notAbsolute":
      return "localAi.err.notAbsolute";
    case "insideProject":
      return "localAi.err.insideProject";
    case "insideStore":
      return "localAi.err.insideStore";
    case "insideAppData":
      return "localAi.err.insideAppData";
    case "empty":
      return "localAi.err.tokenEmpty";
    case "tooLong":
      return "localAi.err.tokenTooLong";
    case "testRunning":
      return "localAi.err.testRunning";
    case "noTest":
      return "localAi.err.noTest";
    default:
      return null;
  }
}

/** What went wrong with a Local AI action, in words. */
export function errorText(error: ApiError): string {
  if (error.kind !== "backend") return apiErrorText(error);
  const key =
    reasonKey(error.error.detail) ??
    (error.error.kind === "unavailable" ? reasonKey(error.error.subject) : null);
  return key === null ? apiErrorText(error) : t(key);
}

/** Why the model engine failed a test (backend `detail` code of subject `engine`) -> its own words. */
function engineKey(detail: string | null): MessageKey | null {
  switch (detail) {
    case "outOfMemory":
      return "localAi.test.failed.outOfMemory";
    case "notAvailable":
      return "localAi.test.failed.notAvailable";
    case "crashed":
    case "notLoaded":
      return "localAi.test.failed.crashed";
    case "blocked":
      return "localAi.test.failed.blocked";
    case "engineTooOld":
    case "unsupported":
      return "localAi.test.failed.needsNewer";
    case "corrupt":
      return "localAi.test.failed.corrupt";
    default:
      return null;
  }
}

/** Why "Test model" failed | was refused, in words (engine failures first, then the page's refusal words). */
export function testErrorText(error: ApiError): string {
  const key =
    error.kind === "backend" && error.error.subject === "engine"
      ? engineKey(error.error.detail)
      : null;
  return key === null ? errorText(error) : t(key);
}

/** A short duration: "0.6 seconds", "1.8 seconds" (one decimal, the current language's unit words). */
export function secondsText(ms: number): string {
  return new Intl.NumberFormat(language.locale, {
    style: "unit",
    unit: "second",
    unitDisplay: "long",
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  }).format(ms / 1000);
}

/** How fast a model writes: "8.6 tokens a second". */
export function speedText(tokensPerSecond: number): string {
  const speed = new Intl.NumberFormat(language.locale, {
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  }).format(tokensPerSecond);
  return t("localAi.test.speed", { speed });
}

/** A store change refused until the person says what happens to the installed models (move | keep). */
export function asksMoveOrKeep(error: ApiError): boolean {
  return (
    error.kind === "backend" &&
    error.error.kind === "conflict" &&
    error.error.detail === "modeRequired"
  );
}
