// Plain words of the Local AI page: jobs, what a model can do, languages, fit hints, sizes, refusals. No jargon: the
// person reads "Search by meaning", "Runs slowly here", "3.4 GB".

import type { ApiError } from "../../api";
import type { DownloadError } from "../../api/generated/DownloadError";
import type { ModelFit } from "../../api/generated/ModelFit";
import { apiErrorText } from "../../core/errors";
import { language, t, tn, type MessageKey } from "../../core/i18n";
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

/** A download | memory size: "310 MB", "3.4 GB" (decimal units, as stores and disks show them). */
export function sizeText(bytes: number): string {
  const giga = bytes >= 1e9;
  return new Intl.NumberFormat(language.locale, {
    style: "unit",
    unit: giga ? "gigabyte" : "megabyte",
    maximumFractionDigits: giga ? 1 : 0,
  }).format(bytes / (giga ? 1e9 : 1e6));
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
