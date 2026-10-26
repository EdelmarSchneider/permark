// The mock's copy of the backend's address rules for web pictures (`permark_core::remote::check_url` + `is_public`):
// the same refusals with the same error, so `dev:mock` answers like the app. A name is never resolved here (the app's
// resolver refuses a name with an address that is not public; the mock has no network).

import type { ErrorKind } from "./generated/ErrorKind";
import type { IpcError } from "./generated/IpcError";

const MAX_URL_CHARS = 2048;

function refusal(kind: ErrorKind, subject: string | null, detail: string): IpcError {
  return { kind, subject, detail };
}

function parseV4(text: string): number[] | null {
  const parts = text.split(".");
  if (parts.length !== 4) return null;
  const octets: number[] = [];
  for (const part of parts) {
    // a plain decimal octet: no leading zero, no sign, <= 255 (as Rust's `Ipv4Addr` parser)
    if (!/^(0|[1-9]\d{0,2})$/.test(part)) return null;
    const value = Number(part);
    if (value > 255) return null;
    octets.push(value);
  }
  return octets;
}

/** The 8 segments of an IPv6 address (`::` and a dotted IPv4 tail allowed), or `null`. */
function parseV6(text: string): number[] | null {
  if (text.includes("%")) return null;
  let tail: number[] = [];
  let body = text;
  const lastColon = text.lastIndexOf(":");
  if (text.slice(lastColon + 1).includes(".")) {
    const v4 = parseV4(text.slice(lastColon + 1));
    if (v4 === null) return null;
    tail = [((v4[0] ?? 0) << 8) | (v4[1] ?? 0), ((v4[2] ?? 0) << 8) | (v4[3] ?? 0)];
    // keep a `::` right before the IPv4 tail, drop a single `:`
    body = text.slice(0, lastColon + 1);
    if (!body.endsWith("::")) body = body.slice(0, -1);
  }
  const halves = body.split("::");
  if (halves.length > 2) return null;
  const group = (part: string): number[] | null => {
    if (part === "") return [];
    const out: number[] = [];
    for (const piece of part.split(":")) {
      if (!/^[0-9a-fA-F]{1,4}$/.test(piece)) return null;
      out.push(parseInt(piece, 16));
    }
    return out;
  };
  const head = group(halves[0] ?? "");
  const rest = halves.length === 2 ? group(halves[1] ?? "") : [];
  if (head === null || rest === null) return null;
  const known = head.length + rest.length + tail.length;
  if (halves.length === 2) {
    if (known > 7) return null;
    return [...head, ...Array<number>(8 - known).fill(0), ...rest, ...tail];
  }
  return known === 8 ? [...head, ...tail] : null;
}

function publicV4([a = 0, b = 0, c = 0]: number[]): boolean {
  const notPublic =
    a === 0 ||
    a === 10 ||
    a === 127 ||
    a >= 224 ||
    (a === 100 && b >= 64 && b < 128) ||
    (a === 169 && b === 254) ||
    (a === 172 && b >= 16 && b < 32) ||
    (a === 192 && b === 168) ||
    (a === 192 && b === 0 && (c === 0 || c === 2)) ||
    (a === 192 && b === 88 && c === 99) ||
    (a === 198 && (b === 18 || b === 19)) ||
    (a === 198 && b === 51 && c === 100) ||
    (a === 203 && b === 0 && c === 113);
  return !notPublic;
}

function v4At(s: number[], i: number): number[] {
  const hi = s[i] ?? 0;
  const lo = s[i + 1] ?? 0;
  return [hi >> 8, hi & 0xff, lo >> 8, lo & 0xff];
}

function zero(s: number[], from: number, to: number): boolean {
  return s.slice(from, to).every((x) => x === 0);
}

function publicV6(s: number[]): boolean {
  const [s0 = 0, s1 = 0, s2 = 0] = s;
  // an IPv6 form that carries an IPv4 address is judged by that address
  if (zero(s, 0, 5) && (s[5] === 0 || s[5] === 0xffff)) return publicV4(v4At(s, 6));
  if (zero(s, 0, 4) && s[4] === 0xffff && s[5] === 0) return publicV4(v4At(s, 6));
  if (s0 === 0x64 && s1 === 0xff9b && zero(s, 2, 6)) return publicV4(v4At(s, 6));
  if (s0 === 0x2002) return publicV4(v4At(s, 1));
  const notPublic =
    (s0 & 0xfe00) === 0xfc00 ||
    (s0 & 0xffc0) === 0xfe80 ||
    (s0 & 0xffc0) === 0xfec0 ||
    (s0 & 0xff00) === 0xff00 ||
    (s0 === 0x100 && zero(s, 1, 4)) ||
    (s0 === 0x2001 && s1 === 0x0db8) ||
    (s0 === 0x2001 && s1 < 0x200 && !ianaGlobal2001(s)) ||
    (s0 === 0x3fff && s1 < 0x1000) ||
    s0 === 0x5f00 ||
    (s0 === 0x64 && s1 === 0xff9b && s2 === 1);
  return !notPublic;
}

/** The globally reachable sub-ranges of `2001::/23` (IANA), as `iana_global_2001` in the backend. */
function ianaGlobal2001(s: number[]): boolean {
  const s1 = s[1] ?? 0;
  const anycast = s1 === 1 && zero(s, 2, 7) && (s[7] ?? 0) >= 1 && (s[7] ?? 0) <= 3;
  return anycast || s1 === 3 || (s1 === 4 && s[2] === 0x112) || (s1 >= 0x20 && s1 < 0x40);
}

function hostPart(authority: string): string {
  if (authority.startsWith("[")) {
    const end = authority.indexOf("]");
    return end < 0 ? authority : authority.slice(0, end + 1);
  }
  return authority.split(":")[0] ?? "";
}

/** Why the backend refuses to ask for the picture at `url` before any request, or `null` (it would ask). */
export function mockPictureRefusal(url: string): IpcError | null {
  const text = url.trim();
  if ([...text].length > MAX_URL_CHARS || /[\s\p{Cc}]/u.test(text)) {
    return refusal("invalidData", null, "address");
  }
  if (text.slice(0, 8).toLowerCase() !== "https://") return refusal("unsupported", null, "https");
  const authority = text.slice(8).split(/[/?#]/)[0] ?? "";
  if (authority === "" || authority.includes("@") || authority.startsWith(":")) {
    return refusal("invalidData", null, "address");
  }
  const host = hostPart(authority);
  const invalid = refusal("invalidData", null, "address");
  const local = refusal("denied", host, "address");
  if (host.startsWith("[")) {
    const segments = host.endsWith("]") ? parseV6(host.slice(1, -1)) : null;
    if (segments === null) return invalid;
    return publicV6(segments) ? null : local;
  }
  // one trailing dot is ignored; any other empty label is refused
  const name = (host.endsWith(".") ? host.slice(0, -1) : host).toLowerCase();
  if (name.split(".").some((label) => label === "")) return invalid;
  if (name === "localhost" || name.endsWith(".localhost")) return local;
  const last = name.split(".").pop() ?? "";
  if (!/^\d/.test(last)) return null;
  const octets = parseV4(name);
  if (octets === null) return invalid;
  return publicV4(octets) ? null : local;
}
