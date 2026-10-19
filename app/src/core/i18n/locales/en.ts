// Source language + fallback of every table. Placeholders: `{name}`. Add | rename | remove a key -> every locale file,
// same change (compile error + parity test enforce it).
const en = {
  "app.name": "PerMark",
  "app.loading": "Loading",

  // ---- welcome
  "welcome.tagline": "Read, navigate and follow your Markdown docs.",

  // ---- status bar
  "status.version": "Version {version}",
  "status.versionUnavailable": "Version unavailable",
} as const;

export default en;
