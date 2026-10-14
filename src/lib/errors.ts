// Turns whatever a rejected backend call carries into a short message for the UI.
// Tauri rejects with the serialized `AppError`, which may be a string or an object with a message.

/** The stable `kind` the backend gave an error (`exists`, `denied`, `notFound`...), or null when it has none. */
export function errorKind(error: unknown): string | null {
  if (error && typeof error === "object") {
    const kind = (error as Record<string, unknown>).kind;
    if (typeof kind === "string") return kind;
  }
  return null;
}

export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  if (error && typeof error === "object") {
    const record = error as Record<string, unknown>;
    for (const key of ["message", "msg", "error", "detail"]) {
      const value = record[key];
      if (typeof value === "string" && value !== "") return value;
    }
    const entries = Object.entries(record);
    // serde externally tagged enums look like { notFound: "path" } or { io: { message } }
    if (entries.length === 1) {
      const [tag, value] = entries[0];
      if (typeof value === "string") return `${tag}: ${value}`;
      if (value && typeof value === "object") return errorText(value);
    }
    try {
      return JSON.stringify(error);
    } catch {
      return String(error);
    }
  }
  return String(error);
}
