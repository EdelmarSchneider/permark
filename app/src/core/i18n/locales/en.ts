// Source language + fallback of every table. Placeholders: `{name}`. Add | rename | remove a key -> every locale file,
// same change (compile error + parity test enforce it).
const en = {
  "app.name": "PerMark",
  "app.loading": "Loading",
  "app.opening": "Opening {name}…",

  // ---- window title
  "window.title.project": "{document} — {project} — PerMark",
  "window.title.projectOnly": "{project} — PerMark",
  "window.title.file": "{document} — PerMark",

  // ---- common
  "common.cancel": "Cancel",
  "common.close": "Close",
  "common.dismiss": "Dismiss",
  "common.details": "Details",

  // ---- dialogs
  "dialog.openFolder": "Open folder",
  "dialog.openFile": "Open file",
  "dialog.filter.docs": "Documents",
  "dialog.windowTitle": "Where should this folder open?",
  "dialog.windowBody":
    "This window replaces what you have open. A new window leaves this one as it is.",
  "dialog.windowHere": "This window",
  "dialog.windowNew": "New window",

  // ---- welcome
  "welcome.tagline": "Read, navigate and follow your Markdown docs.",
  "welcome.openFolder": "Open folder...",
  "welcome.openFile": "Open file...",
  "welcome.recent": "Recent folders",
  "welcome.noRecents": "No recent folders yet.",
  "welcome.notFound": "(not found)",
  "welcome.remove": "Remove",
  "welcome.dropHint": "Drop a folder or a .md file anywhere to open it.",
  "welcome.dropActive": "Drop to open. A folder opens as a project, a file on its own.",
  "welcome.dropOne": "Drop one folder or one file.",
  "welcome.openFailed": "Can't open {path}: {message}",

  // ---- header + workspaces
  "header.close": "Close",
  "header.closeHint": "Close and go back to the start screen",
  "header.settings": "Settings",
  "project.explorer": "Explorer",
  "project.panel": "Panel",
  "project.noDocument": "Choose a document to read.",
  "project.document": "Document: {path}",
  "file.folder": "In {folder}",

  // ---- status bar
  "status.version": "Version {version}",
  "status.versionUnavailable": "Version unavailable",

  // ---- notices
  "notice.stateRecovered": "Some saved PerMark data couldn't be read and was set aside: {file}",

  // ---- settings
  "settings.title": "Settings",
  "settings.language": "Language",
  "settings.languageSystem": "System default",
  "settings.theme": "Theme",
  "settings.themeSystem": "System",
  "settings.themeLight": "Light",
  "settings.themeDark": "Dark",
  "settings.tone": "Light tone",
  "settings.toneWhite": "White",
  "settings.tonePaper": "Paper",
  "settings.toneSage": "Sage",
  "settings.toneGray": "Gray",
  "settings.notSaved": "Couldn't keep this setting for next time: {message}",

  // ---- errors (one per backend error kind)
  "error.notFound": "Not found: {subject}",
  "error.pathEscape": "{subject} is outside the open folder.",
  "error.invalidPath": "{subject} isn't a valid path.",
  "error.denied": "No permission to use {subject}.",
  "error.io": "Couldn't access {subject}: {detail}",
  "error.exists": "{subject} already exists.",
  "error.binary": "{subject} is a binary file, so it can't be shown as text.",
  "error.tooLarge": "{subject} is too large.",
  "error.encoding": "{subject} isn't text PerMark can read.",
  "error.unsupported": "This isn't available here: {subject}",
  "error.invalidData": "The data isn't valid: {subject}",
  "error.conflict": "{subject} changed in the meantime.",
  "error.unavailable": "This isn't available right now.",
  "error.superseded": "Something else was opened in the meantime.",
  "error.internal": "Something went wrong inside PerMark: {detail}",
  "error.unknown": "Something went wrong: {message}",

  // ---- conditions: why a feature can't work here (one plain sentence) + the fix
  "condition.workspace": "This works in another kind of window.",
  "condition.gitRepo": "This folder isn't a git repository.",
  "condition.gitAvailable": "Git isn't available on this computer.",
  "condition.permark": "PerMark isn't set up in this folder yet.",
  "condition.agentOn": "The agent connection is off for this project.",
  "condition.agentConnected": "No agent is connected right now.",
  "condition.liveUpdates": "Live updates aren't available for this folder.",
  "condition.distribution": "This build of PerMark doesn't include this.",
  "condition.platform": "This isn't available on this operating system.",
  "condition.plugin": "The plugin for this is turned off.",
  "condition.remedy.project.init": "Set up PerMark here",
  "condition.remedy.agent.turnOn": "Turn on the agent connection",
  "condition.remedy.plugins.enable": "Turn the plugin on",
  "condition.remedy.settings.git": "Git settings",
} as const;

export default en;
