// The formats that read as a sheet (ids as the backend names them: `FormatId`).

export const SHEET_FORMATS: ReadonlySet<string> = new Set([
  "toml",
  "yaml",
  "ini",
  "dotenv",
  "properties",
  "ignore",
  "toon",
]);
