// Español. Labels as infinitives or imperatives ("Abrir carpeta"); messages impersonal; informal "tú" in hints.
import type { Messages } from "../messages";

export default {
  "app.name": "PerMark",
  "app.loading": "Cargando",
  "app.opening": "Abriendo {name}…",

  // ---- window title
  "window.title.project": "{document} — {project} — PerMark",
  "window.title.projectOnly": "{project} — PerMark",
  "window.title.file": "{document} — PerMark",

  // ---- common
  "common.cancel": "Cancelar",
  "common.close": "Cerrar",
  "common.dismiss": "Descartar",
  "common.details": "Detalles",

  // ---- dialogs
  "dialog.openFolder": "Abrir carpeta",
  "dialog.openFile": "Abrir archivo",
  "dialog.filter.docs": "Documentos",
  "dialog.windowTitle": "¿Dónde se abre esta carpeta?",
  "dialog.windowBody":
    "Esta ventana sustituye lo que tienes abierto. Una ventana nueva deja esta como está.",
  "dialog.windowHere": "Esta ventana",
  "dialog.windowNew": "Ventana nueva",

  // ---- welcome
  "welcome.tagline": "Lee, navega y sigue tus documentos Markdown.",
  "welcome.openFolder": "Abrir carpeta...",
  "welcome.openFile": "Abrir archivo...",
  "welcome.recent": "Carpetas recientes",
  "welcome.noRecents": "Aún no hay carpetas recientes.",
  "welcome.notFound": "(no se encontró)",
  "welcome.remove": "Quitar",
  "welcome.dropHint": "Suelta una carpeta o un archivo .md en cualquier parte para abrirlo.",
  "welcome.dropActive":
    "Suelta para abrir. Una carpeta se abre como proyecto; un archivo, por sí solo.",
  "welcome.dropOne": "Suelta solo una carpeta o un archivo.",
  "welcome.openFailed": "No se pudo abrir {path}: {message}",

  // ---- header + workspaces
  "header.close": "Cerrar",
  "header.closeHint": "Cerrar y volver a la pantalla de inicio",
  "header.settings": "Configuración",
  "project.explorer": "Explorador",
  "project.panel": "Panel",
  "project.noDocument": "Elige un documento para leer.",
  "project.document": "Documento: {path}",
  "file.folder": "En {folder}",

  // ---- status bar
  "status.version": "Versión {version}",
  "status.versionUnavailable": "Versión no disponible",

  // ---- notices
  "notice.stateRecovered":
    "Algunos datos guardados de PerMark no se pudieron leer y se apartaron: {file}",

  // ---- settings
  "settings.title": "Configuración",
  "settings.language": "Idioma",
  "settings.languageSystem": "Predeterminado del sistema",
  "settings.theme": "Tema",
  "settings.themeSystem": "Sistema",
  "settings.themeLight": "Claro",
  "settings.themeDark": "Oscuro",
  "settings.tone": "Tono claro",
  "settings.toneWhite": "Blanco",
  "settings.tonePaper": "Papel",
  "settings.toneSage": "Salvia",
  "settings.toneGray": "Gris",
  "settings.notSaved": "No se pudo guardar este ajuste para la próxima vez: {message}",

  // ---- errors
  "error.notFound": "No se encontró: {subject}",
  "error.pathEscape": "{subject} está fuera de la carpeta abierta.",
  "error.invalidPath": "{subject} no es una ruta válida.",
  "error.denied": "Sin permiso para usar {subject}.",
  "error.io": "No se pudo acceder a {subject}: {detail}",
  "error.exists": "{subject} ya existe.",
  "error.binary": "{subject} es un archivo binario y no se puede mostrar como texto.",
  "error.tooLarge": "{subject} es demasiado grande.",
  "error.encoding": "{subject} no es un texto que PerMark pueda leer.",
  "error.unsupported": "Esto no está disponible aquí: {subject}",
  "error.invalidData": "Los datos no son válidos: {subject}",
  "error.conflict": "{subject} cambió mientras tanto.",
  "error.unavailable": "Esto no está disponible ahora.",
  "error.superseded": "Mientras tanto se abrió otra cosa.",
  "error.internal": "Algo salió mal dentro de PerMark: {detail}",
  "error.unknown": "Algo salió mal: {message}",

  // ---- conditions
  "condition.workspace": "Esto funciona en otro tipo de ventana.",
  "condition.gitRepo": "Esta carpeta no es un repositorio git.",
  "condition.gitAvailable": "Git no está disponible en este equipo.",
  "condition.permark": "PerMark aún no está configurado en esta carpeta.",
  "condition.agentOn": "La conexión con agentes está desactivada en este proyecto.",
  "condition.agentConnected": "No hay ningún agente conectado ahora.",
  "condition.liveUpdates": "Las actualizaciones en vivo no están disponibles para esta carpeta.",
  "condition.distribution": "Esta versión de PerMark no incluye esto.",
  "condition.platform": "Esto no está disponible en este sistema operativo.",
  "condition.plugin": "El plugin para esto está desactivado.",
  "condition.remedy.project.init": "Configurar PerMark aquí",
  "condition.remedy.agent.turnOn": "Activar la conexión con agentes",
  "condition.remedy.plugins.enable": "Activar el plugin",
  "condition.remedy.settings.git": "Ajustes de git",
} satisfies Messages;
