// Names the mock backend accepts for new and renamed files and folders: the backend's rules (`fileops/names.rs`,
// `paths::windows_name_ok`), the same on every system. Tests and dev:mock only; the UI's own quick check while typing
// lives with the Explorer and is not this.

/** What is wrong with a name: the `detail` of an `invalidName` refusal. */
export type MockNameProblem =
  "empty" | "chars" | "edge" | "dot" | "reserved" | "tooLong" | "notMarkdown";

/** Longest name in UTF-8 bytes. */
const MAX_NAME = 255;

const DEVICES = new Set(["CON", "PRN", "AUX", "NUL", "CONIN$", "CONOUT$"]);
const DEVICE_DIGITS = new Set(["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "¹", "²", "³"]);

/** A name Windows keeps for a device, whatever the extension (`nul.txt`). */
function reservedDevice(name: string): boolean {
  // ASCII upper case only, as the backend
  const stem = (name.split(".")[0] ?? name).trimEnd().replace(/[a-z]/g, (c) => c.toUpperCase());
  if (DEVICES.has(stem)) return true;
  const chars = Array.from(stem);
  if (chars.length !== 4) return false;
  const base = chars.slice(0, 3).join("");
  return (base === "COM" || base === "LPT") && DEVICE_DIGITS.has(chars[3] ?? "");
}

/** A control character, as Rust's `char::is_control` (category Cc). */
function isControl(c: string): boolean {
  const code = c.codePointAt(0) ?? 0;
  return code < 0x20 || (code >= 0x7f && code <= 0x9f);
}

/** The first problem of a file or folder name, or `null`. */
export function checkName(name: string): MockNameProblem | null {
  if (name.trim() === "") return "empty";
  if (name === "." || name === "..") return "dot";
  if (/[\\/:*?"<>|]/.test(name) || Array.from(name).some(isControl)) return "chars";
  if (name.startsWith(" ") || name.endsWith(" ") || name.endsWith(".")) return "edge";
  if (new TextEncoder().encode(name).length > MAX_NAME) return "tooLong";
  if (reservedDevice(name)) return "reserved";
  return null;
}

/** The extension in lower case: none for `.hidden`, `name.` or no dot. */
export function extensionOf(name: string): string | null {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot + 1 === name.length) return null;
  return name.slice(dot + 1).toLowerCase();
}

/** The name of a new file: no extension gets `.md`; only Markdown and plain text are made. */
export function newFileName(
  name: string,
): { ok: true; name: string } | { ok: false; problem: MockNameProblem } {
  const first = checkName(name);
  if (first !== null) return { ok: false, problem: first };
  const ext = extensionOf(name);
  let full: string;
  if (ext === null && !name.startsWith(".")) full = `${name}.md`;
  else if (ext === "md" || ext === "markdown" || ext === "txt") full = name;
  else return { ok: false, problem: "notMarkdown" };
  const problem = checkName(full);
  return problem === null ? { ok: true, name: full } : { ok: false, problem };
}
