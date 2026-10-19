// Português (Brasil). Labels imperative ("Salvar"); messages impersonal; never "senhor/senhora". Git terms stay English.
import type { Messages } from "../messages";

export default {
  "app.name": "PerMark",
  "app.loading": "Carregando",
  "app.opening": "Abrindo {name}…",

  // ---- window title
  "window.title.project": "{document} — {project} — PerMark",
  "window.title.projectOnly": "{project} — PerMark",
  "window.title.file": "{document} — PerMark",

  // ---- common
  "common.cancel": "Cancelar",
  "common.close": "Fechar",
  "common.dismiss": "Ignorar",
  "common.details": "Detalhes",

  // ---- dialogs
  "dialog.openFolder": "Abrir pasta",
  "dialog.openFile": "Abrir arquivo",
  "dialog.filter.docs": "Documentos",
  "dialog.windowTitle": "Onde esta pasta deve abrir?",
  "dialog.windowBody": "Esta janela troca o que está aberto. Uma janela nova deixa esta como está.",
  "dialog.windowHere": "Esta janela",
  "dialog.windowNew": "Janela nova",

  // ---- welcome
  "welcome.tagline": "Leia, navegue e acompanhe seus documentos Markdown.",
  "welcome.openFolder": "Abrir pasta...",
  "welcome.openFile": "Abrir arquivo...",
  "welcome.recent": "Pastas recentes",
  "welcome.noRecents": "Nenhuma pasta recente ainda.",
  "welcome.notFound": "(não encontrada)",
  "welcome.remove": "Remover",
  "welcome.dropHint": "Solte uma pasta ou um arquivo .md em qualquer lugar para abri-lo.",
  "welcome.dropActive": "Solte para abrir. Uma pasta abre como projeto; um arquivo, sozinho.",
  "welcome.dropOne": "Solte apenas uma pasta ou um arquivo.",
  "welcome.openFailed": "Não foi possível abrir {path}: {message}",

  // ---- header + workspaces
  "header.close": "Fechar",
  "header.closeHint": "Fechar e voltar à tela inicial",
  "header.settings": "Configurações",
  "project.explorer": "Explorador",
  "project.panel": "Painel",
  "project.noDocument": "Escolha um documento para ler.",
  "project.document": "Documento: {path}",
  "file.folder": "Em {folder}",

  // ---- status bar
  "status.version": "Versão {version}",
  "status.versionUnavailable": "Versão indisponível",

  // ---- notices
  "notice.stateRecovered":
    "Alguns dados salvos do PerMark não puderam ser lidos e foram guardados à parte: {file}",

  // ---- settings
  "settings.title": "Configurações",
  "settings.language": "Idioma",
  "settings.languageSystem": "Padrão do sistema",
  "settings.theme": "Tema",
  "settings.themeSystem": "Sistema",
  "settings.themeLight": "Claro",
  "settings.themeDark": "Escuro",
  "settings.tone": "Tom claro",
  "settings.toneWhite": "Branco",
  "settings.tonePaper": "Papel",
  "settings.toneSage": "Sálvia",
  "settings.toneGray": "Cinza",
  "settings.notSaved": "Não foi possível guardar esta configuração para a próxima vez: {message}",

  // ---- errors
  "error.notFound": "Não encontrado: {subject}",
  "error.pathEscape": "{subject} está fora da pasta aberta.",
  "error.invalidPath": "{subject} não é um caminho válido.",
  "error.denied": "Sem permissão para usar {subject}.",
  "error.io": "Não foi possível acessar {subject}: {detail}",
  "error.exists": "{subject} já existe.",
  "error.binary": "{subject} é um arquivo binário e não pode ser mostrado como texto.",
  "error.tooLarge": "{subject} é grande demais.",
  "error.encoding": "{subject} não é um texto que o PerMark consiga ler.",
  "error.unsupported": "Isto não está disponível aqui: {subject}",
  "error.invalidData": "Os dados não são válidos: {subject}",
  "error.conflict": "{subject} mudou nesse meio-tempo.",
  "error.unavailable": "Isto não está disponível agora.",
  "error.superseded": "Outra coisa foi aberta nesse meio-tempo.",
  "error.internal": "Algo deu errado dentro do PerMark: {detail}",
  "error.unknown": "Algo deu errado: {message}",

  // ---- conditions
  "condition.workspace": "Isto funciona em outro tipo de janela.",
  "condition.gitRepo": "Esta pasta não é um repositório git.",
  "condition.gitAvailable": "O git não está disponível neste computador.",
  "condition.permark": "O PerMark ainda não foi configurado nesta pasta.",
  "condition.agentOn": "A conexão com agentes está desligada neste projeto.",
  "condition.agentConnected": "Nenhum agente está conectado agora.",
  "condition.liveUpdates": "Atualizações ao vivo não estão disponíveis para esta pasta.",
  "condition.distribution": "Esta versão do PerMark não inclui isto.",
  "condition.platform": "Isto não está disponível neste sistema operacional.",
  "condition.plugin": "O plugin para isto está desligado.",
  "condition.remedy.project.init": "Configurar o PerMark aqui",
  "condition.remedy.agent.turnOn": "Ligar a conexão com agentes",
  "condition.remedy.plugins.enable": "Ligar o plugin",
  "condition.remedy.settings.git": "Configurações do git",
} satisfies Messages;
