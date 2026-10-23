// A link of a single file that leads above its folder: where it points on disk, and the nearest folder that holds both
// the page and that place (opened as a project, it follows the link). Pure: paths in, paths out; nothing is read.

/** The folder to open and the document inside it the link names. */
export type OutsideTarget = {
  /** The nearest folder holding both the page and the link's target (absolute, as the system writes it). */
  folder: string;
  /** Its name, for the button. */
  name: string;
  /** The target inside it (`/` separators). */
  document: string;
  anchor: string | null;
};

/** Segments of an absolute path (`C:` | `` first, then the names), or `null` when it is not absolute. */
function segmentsOf(path: string): string[] | null {
  const slashed = path.replace(/\\/g, "/");
  if (/^[A-Za-z]:\//.test(slashed) || slashed.startsWith("/")) {
    const [head = "", ...rest] = slashed.split("/");
    return [head, ...rest.filter((s) => s !== "" && s !== ".")];
  }
  return null;
}

function decode(text: string): string {
  try {
    return decodeURIComponent(text);
  } catch {
    return text;
  }
}

/**
 * Where a relative `href` of the document `doc` (a path inside the single file's `folder`) leads, and the nearest
 * folder holding both. `null` when the link is not relative, or the only folder holding both is a drive | file system
 * root (opening a whole drive as a project would read all of it: no button can reach the target sensibly).
 */
export function outsideTarget(folder: string, doc: string, href: string): OutsideTarget | null {
  const [beforeHash = "", ...hash] = href.trim().split("#");
  const anchor = hash.length > 0 && hash.join("#") !== "" ? decode(hash.join("#")) : null;
  const target = decode(beforeHash.split("?")[0] ?? "").replace(/\\/g, "/");
  if (target === "" || target.startsWith("/") || /^[a-z][a-z0-9+.-]*:/i.test(target)) return null;
  const base = segmentsOf(folder);
  if (base === null) return null;
  const page = [
    ...base,
    ...doc
      .split("/")
      .filter((s) => s !== "")
      .slice(0, -1),
  ];
  const parts = [...page];
  for (const part of target.split("/")) {
    if (part === "" || part === ".") continue;
    if (part === "..") {
      // nothing above the root of the drive
      if (parts.length <= 1) return null;
      parts.pop();
    } else parts.push(part);
  }
  const fileParts = parts.slice(0, -1);
  let common = 0;
  const caseless = /^[A-Za-z]:$/.test(base[0] ?? "");
  const same = (a: string, b: string): boolean =>
    caseless ? a.toLowerCase() === b.toLowerCase() : a === b;
  while (
    common < page.length &&
    common < fileParts.length &&
    same(page[common] ?? "", fileParts[common] ?? "")
  )
    common++;
  // the root itself (`C:` | ``) is no folder to open
  if (common <= 1) return null;
  const holder = parts.slice(0, common);
  const head = holder[0] ?? "";
  const folderPath = head === "" ? `/${holder.slice(1).join("/")}` : holder.join("/");
  return {
    folder: folderPath,
    name: holder[holder.length - 1] ?? folderPath,
    document: parts.slice(common).join("/"),
    anchor,
  };
}
