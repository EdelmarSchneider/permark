// Deutsch. Labels as nouns or infinitives ("Ordner öffnen"); messages impersonal; hints address nobody (no "Sie"/"du").
import type { Messages } from "../messages";

export default {
  "app.name": "PerMark",
  "app.loading": "Wird geladen",
  "app.opening": "{name} wird geöffnet …",

  // ---- window title
  "window.title.project": "{document} — {project} — PerMark",
  "window.title.projectOnly": "{project} — PerMark",
  "window.title.file": "{document} — PerMark",

  // ---- common
  "common.cancel": "Abbrechen",
  "common.close": "Schließen",
  "common.dismiss": "Ausblenden",
  "common.details": "Details",

  // ---- dialogs
  "dialog.openFolder": "Ordner öffnen",
  "dialog.openFile": "Datei öffnen",
  "dialog.filter.docs": "Dokumente",
  "dialog.windowTitle": "Wo soll dieser Ordner aufgehen?",
  "dialog.windowBody":
    "Dieses Fenster ersetzt, was gerade offen ist. Ein neues Fenster lässt dieses, wie es ist.",
  "dialog.windowHere": "Dieses Fenster",
  "dialog.windowNew": "Neues Fenster",

  // ---- welcome
  "welcome.tagline": "Markdown-Dokumente lesen, durchsuchen und verfolgen.",
  "welcome.openFolder": "Ordner öffnen ...",
  "welcome.openFile": "Datei öffnen ...",
  "welcome.recent": "Zuletzt geöffnete Ordner",
  "welcome.noRecents": "Noch keine zuletzt geöffneten Ordner.",
  "welcome.notFound": "(nicht gefunden)",
  "welcome.remove": "Entfernen",
  "welcome.dropHint": "Ordner oder .md-Datei an eine beliebige Stelle ziehen, um sie zu öffnen.",
  "welcome.dropActive":
    "Zum Öffnen loslassen. Ein Ordner öffnet sich als Projekt, eine Datei für sich.",
  "welcome.dropOne": "Nur einen Ordner oder eine Datei loslassen.",
  "welcome.openFailed": "{path} konnte nicht geöffnet werden: {message}",

  // ---- header + workspaces
  "header.close": "Schließen",
  "header.closeHint": "Schließen und zum Startbildschirm zurückkehren",
  "header.settings": "Einstellungen",
  "project.explorer": "Explorer",
  "project.panel": "Bereich",
  "project.noDocument": "Ein Dokument zum Lesen auswählen.",
  "project.document": "Dokument: {path}",
  "file.folder": "In {folder}",

  // ---- status bar
  "status.version": "Version {version}",
  "status.versionUnavailable": "Version nicht verfügbar",

  // ---- notices
  "notice.stateRecovered":
    "Einige gespeicherte PerMark-Daten konnten nicht gelesen werden und wurden beiseitegelegt: {file}",

  // ---- settings
  "settings.title": "Einstellungen",
  "settings.language": "Sprache",
  "settings.languageSystem": "Systemstandard",
  "settings.theme": "Design",
  "settings.themeSystem": "System",
  "settings.themeLight": "Hell",
  "settings.themeDark": "Dunkel",
  "settings.tone": "Farbton (Hell)",
  "settings.toneWhite": "Weiß",
  "settings.tonePaper": "Papier",
  "settings.toneSage": "Salbei",
  "settings.toneGray": "Grau",
  "settings.notSaved":
    "Diese Einstellung konnte nicht für das nächste Mal gespeichert werden: {message}",

  // ---- errors
  "error.notFound": "Nicht gefunden: {subject}",
  "error.pathEscape": "{subject} liegt außerhalb des geöffneten Ordners.",
  "error.invalidPath": "{subject} ist kein gültiger Pfad.",
  "error.denied": "Keine Berechtigung für {subject}.",
  "error.io": "Zugriff auf {subject} nicht möglich: {detail}",
  "error.exists": "{subject} existiert bereits.",
  "error.binary": "{subject} ist eine Binärdatei und kann nicht als Text angezeigt werden.",
  "error.tooLarge": "{subject} ist zu groß.",
  "error.encoding": "{subject} ist kein Text, den PerMark lesen kann.",
  "error.unsupported": "Hier nicht verfügbar: {subject}",
  "error.invalidData": "Die Daten sind ungültig: {subject}",
  "error.conflict": "{subject} wurde inzwischen geändert.",
  "error.unavailable": "Gerade nicht verfügbar.",
  "error.superseded": "Inzwischen wurde etwas anderes geöffnet.",
  "error.internal": "In PerMark ist etwas schiefgelaufen: {detail}",
  "error.unknown": "Etwas ist schiefgelaufen: {message}",

  // ---- conditions
  "condition.workspace": "Das funktioniert in einer anderen Art von Fenster.",
  "condition.gitRepo": "Dieser Ordner ist kein git-Repository.",
  "condition.gitAvailable": "Git ist auf diesem Computer nicht verfügbar.",
  "condition.permark": "PerMark ist in diesem Ordner noch nicht eingerichtet.",
  "condition.agentOn": "Die Agent-Verbindung ist für dieses Projekt aus.",
  "condition.agentConnected": "Gerade ist kein Agent verbunden.",
  "condition.liveUpdates": "Live-Aktualisierungen sind für diesen Ordner nicht verfügbar.",
  "condition.distribution": "Diese PerMark-Ausgabe enthält das nicht.",
  "condition.platform": "Auf diesem Betriebssystem nicht verfügbar.",
  "condition.plugin": "Das Plugin dafür ist ausgeschaltet.",
  "condition.remedy.project.init": "PerMark hier einrichten",
  "condition.remedy.agent.turnOn": "Agent-Verbindung einschalten",
  "condition.remedy.plugins.enable": "Plugin einschalten",
  "condition.remedy.settings.git": "Git-Einstellungen",
} satisfies Messages;
