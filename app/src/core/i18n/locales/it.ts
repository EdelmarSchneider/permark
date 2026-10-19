// Italiano. Labels as infinitives ("Apri cartella" imperative for buttons); messages impersonal; informal "tu" in hints.
import type { Messages } from "../messages";

export default {
  "app.name": "PerMark",
  "app.loading": "Caricamento",
  "app.opening": "Apertura di {name}…",

  // ---- window title
  "window.title.project": "{document} — {project} — PerMark",
  "window.title.projectOnly": "{project} — PerMark",
  "window.title.file": "{document} — PerMark",

  // ---- common
  "common.cancel": "Annulla",
  "common.close": "Chiudi",
  "common.dismiss": "Ignora",
  "common.details": "Dettagli",

  // ---- dialogs
  "dialog.openFolder": "Apri cartella",
  "dialog.openFile": "Apri file",
  "dialog.filter.docs": "Documenti",
  "dialog.windowTitle": "Dove si apre questa cartella?",
  "dialog.windowBody":
    "Questa finestra sostituisce ciò che hai aperto. Una finestra nuova lascia questa com'è.",
  "dialog.windowHere": "Questa finestra",
  "dialog.windowNew": "Finestra nuova",

  // ---- welcome
  "welcome.tagline": "Leggi, naviga e segui i tuoi documenti Markdown.",
  "welcome.openFolder": "Apri cartella...",
  "welcome.openFile": "Apri file...",
  "welcome.recent": "Cartelle recenti",
  "welcome.noRecents": "Ancora nessuna cartella recente.",
  "welcome.notFound": "(non trovata)",
  "welcome.remove": "Rimuovi",
  "welcome.dropHint": "Trascina una cartella o un file .md in un punto qualsiasi per aprirlo.",
  "welcome.dropActive": "Rilascia per aprire. Una cartella si apre come progetto, un file da solo.",
  "welcome.dropOne": "Rilascia una sola cartella o un solo file.",
  "welcome.openFailed": "Impossibile aprire {path}: {message}",

  // ---- header + workspaces
  "header.close": "Chiudi",
  "header.closeHint": "Chiudi e torna alla schermata iniziale",
  "header.settings": "Impostazioni",
  "project.explorer": "Esplora",
  "project.panel": "Pannello",
  "project.noDocument": "Scegli un documento da leggere.",
  "project.document": "Documento: {path}",
  "file.folder": "In {folder}",

  // ---- status bar
  "status.version": "Versione {version}",
  "status.versionUnavailable": "Versione non disponibile",

  // ---- notices
  "notice.stateRecovered":
    "Alcuni dati salvati di PerMark non sono leggibili e sono stati messi da parte: {file}",

  // ---- settings
  "settings.title": "Impostazioni",
  "settings.language": "Lingua",
  "settings.languageSystem": "Predefinita di sistema",
  "settings.theme": "Tema",
  "settings.themeSystem": "Sistema",
  "settings.themeLight": "Chiaro",
  "settings.themeDark": "Scuro",
  "settings.tone": "Tonalità chiara",
  "settings.toneWhite": "Bianco",
  "settings.tonePaper": "Carta",
  "settings.toneSage": "Salvia",
  "settings.toneGray": "Grigio",
  "settings.notSaved": "Impossibile ricordare questa impostazione per la prossima volta: {message}",

  // ---- errors
  "error.notFound": "Non trovato: {subject}",
  "error.pathEscape": "{subject} è fuori dalla cartella aperta.",
  "error.invalidPath": "{subject} non è un percorso valido.",
  "error.denied": "Nessuna autorizzazione per usare {subject}.",
  "error.io": "Impossibile accedere a {subject}: {detail}",
  "error.exists": "{subject} esiste già.",
  "error.binary": "{subject} è un file binario, quindi non può essere mostrato come testo.",
  "error.tooLarge": "{subject} è troppo grande.",
  "error.encoding": "{subject} non è un testo che PerMark sa leggere.",
  "error.unsupported": "Non disponibile qui: {subject}",
  "error.invalidData": "I dati non sono validi: {subject}",
  "error.conflict": "{subject} è cambiato nel frattempo.",
  "error.unavailable": "Non disponibile in questo momento.",
  "error.superseded": "Nel frattempo è stato aperto qualcos'altro.",
  "error.internal": "Si è verificato un errore in PerMark: {detail}",
  "error.unknown": "Qualcosa è andato storto: {message}",

  // ---- conditions
  "condition.workspace": "Questo funziona in un altro tipo di finestra.",
  "condition.gitRepo": "Questa cartella non è un repository git.",
  "condition.gitAvailable": "Git non è disponibile su questo computer.",
  "condition.permark": "PerMark non è ancora configurato in questa cartella.",
  "condition.agentOn": "La connessione con gli agenti è spenta per questo progetto.",
  "condition.agentConnected": "Nessun agente è connesso in questo momento.",
  "condition.liveUpdates":
    "Gli aggiornamenti in tempo reale non sono disponibili per questa cartella.",
  "condition.distribution": "Questa versione di PerMark non lo include.",
  "condition.platform": "Non disponibile su questo sistema operativo.",
  "condition.plugin": "Il plugin per questo è spento.",
  "condition.remedy.project.init": "Configura PerMark qui",
  "condition.remedy.agent.turnOn": "Accendi la connessione con gli agenti",
  "condition.remedy.plugins.enable": "Accendi il plugin",
  "condition.remedy.settings.git": "Impostazioni di git",
} satisfies Messages;
