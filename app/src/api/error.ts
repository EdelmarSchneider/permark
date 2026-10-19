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
