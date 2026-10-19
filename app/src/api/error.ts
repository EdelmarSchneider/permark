import type { ErrorKind } from "./generated/ErrorKind";
import type { IpcError } from "./generated/IpcError";

/** Any rejection value -> one readable line. Never throws (cyclic | hostile values included). */
export function errorMessage(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  try {
    const text = JSON.stringify(error);
    return typeof text === "string" ? text : String(error);
  } catch {
    return "unknown error";
  }
}

/** Every backend error kind (mirror of Rust `ErrorKind::ALL`; `satisfies` keeps it exhaustive). */
export const ERROR_KINDS = [
  "notFound",
  "pathEscape",
  "invalidPath",
  "denied",
  "io",
  "exists",
  "binary",
  "tooLarge",
  "encoding",
  "unsupported",
  "invalidData",
  "conflict",
  "unavailable",
  "superseded",
  "internal",
] as const satisfies readonly ErrorKind[];

type MissingKinds = Exclude<ErrorKind, (typeof ERROR_KINDS)[number]>;
export const ERROR_KINDS_COMPLETE: MissingKinds extends never ? true : false = true;

function isKind(value: unknown): value is ErrorKind {
  return ERROR_KINDS.some((kind) => kind === value);
}

function optionalText(value: unknown): string | null | undefined {
  if (value === null || value === undefined) return null;
  return typeof value === "string" ? value : undefined;
}

/** Parses a rejection into the backend's `IpcError` shape; anything else -> null. */
export function asIpcError(value: unknown): IpcError | null {
  if (typeof value !== "object" || value === null) return null;
  const kind: unknown = Reflect.get(value, "kind");
  const subject = optionalText(Reflect.get(value, "subject"));
  const detail = optionalText(Reflect.get(value, "detail"));
  if (!isKind(kind) || subject === undefined || detail === undefined) return null;
  return { kind, subject, detail };
}
