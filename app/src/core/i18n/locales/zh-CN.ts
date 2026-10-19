// 简体中文 (Simplified Chinese). Buttons as verb phrases ("打开文件夹"); hints short, no "您"; full-width punctuation.
import type { Messages } from "../messages";

export default {
  "app.name": "PerMark",
  "app.loading": "正在加载",
  "app.opening": "正在打开 {name}…",

  // ---- window title
  "window.title.project": "{document} — {project} — PerMark",
  "window.title.projectOnly": "{project} — PerMark",
  "window.title.file": "{document} — PerMark",

  // ---- common
  "common.cancel": "取消",
  "common.close": "关闭",
  "common.dismiss": "忽略",
  "common.details": "详细信息",

  // ---- dialogs
  "dialog.openFolder": "打开文件夹",
  "dialog.openFile": "打开文件",
  "dialog.filter.docs": "文档",
  "dialog.windowTitle": "这个文件夹在哪里打开？",
  "dialog.windowBody": "此窗口会替换当前打开的内容。新窗口则保留此窗口不变。",
  "dialog.windowHere": "此窗口",
  "dialog.windowNew": "新窗口",

  // ---- welcome
  "welcome.tagline": "阅读、浏览并跟随你的 Markdown 文档。",
  "welcome.openFolder": "打开文件夹...",
  "welcome.openFile": "打开文件...",
  "welcome.recent": "最近的文件夹",
  "welcome.noRecents": "还没有最近打开的文件夹。",
  "welcome.notFound": "（未找到）",
  "welcome.remove": "移除",
  "welcome.dropHint": "将文件夹或 .md 文件拖放到任意位置即可打开。",
  "welcome.dropActive": "松开即可打开。文件夹作为项目打开，文件单独打开。",
  "welcome.dropOne": "一次只能拖入一个文件夹或一个文件。",
  "welcome.openFailed": "无法打开 {path}：{message}",

  // ---- header + workspaces
  "header.close": "关闭",
  "header.closeHint": "关闭并返回开始界面",
  "header.settings": "设置",
  "project.explorer": "资源管理器",
  "project.panel": "面板",
  "project.noDocument": "选择一个文档来阅读。",
  "project.document": "文档：{path}",
  "file.folder": "位于 {folder}",

  // ---- status bar
  "status.version": "版本 {version}",
  "status.versionUnavailable": "版本不可用",

  // ---- notices
  "notice.stateRecovered": "部分已保存的 PerMark 数据无法读取，已另行保留：{file}",

  // ---- settings
  "settings.title": "设置",
  "settings.language": "语言",
  "settings.languageSystem": "跟随系统",
  "settings.theme": "主题",
  "settings.themeSystem": "跟随系统",
  "settings.themeLight": "浅色",
  "settings.themeDark": "深色",
  "settings.tone": "浅色色调",
  "settings.toneWhite": "白色",
  "settings.tonePaper": "纸张",
  "settings.toneSage": "鼠尾草绿",
  "settings.toneGray": "灰色",
  "settings.notSaved": "无法保存此设置供下次使用：{message}",

  // ---- errors
  "error.notFound": "未找到：{subject}",
  "error.pathEscape": "{subject} 位于已打开的文件夹之外。",
  "error.invalidPath": "{subject} 不是有效的路径。",
  "error.denied": "没有使用 {subject} 的权限。",
  "error.io": "无法访问 {subject}：{detail}",
  "error.exists": "{subject} 已存在。",
  "error.binary": "{subject} 是二进制文件，无法作为文本显示。",
  "error.tooLarge": "{subject} 过大。",
  "error.encoding": "{subject} 不是 PerMark 能读取的文本。",
  "error.unsupported": "此处不可用：{subject}",
  "error.invalidData": "数据无效：{subject}",
  "error.conflict": "{subject} 在此期间已更改。",
  "error.unavailable": "目前不可用。",
  "error.superseded": "在此期间打开了其他内容。",
  "error.internal": "PerMark 内部出错：{detail}",
  "error.unknown": "出现问题：{message}",

  // ---- conditions
  "condition.workspace": "此功能适用于另一种窗口。",
  "condition.gitRepo": "这个文件夹不是 git 仓库。",
  "condition.gitAvailable": "此电脑上没有可用的 git。",
  "condition.permark": "尚未在此文件夹中设置 PerMark。",
  "condition.agentOn": "此项目的代理连接已关闭。",
  "condition.agentConnected": "目前没有已连接的代理。",
  "condition.liveUpdates": "此文件夹无法使用实时更新。",
  "condition.distribution": "此版本的 PerMark 不包含此功能。",
  "condition.platform": "此操作系统上不可用。",
  "condition.plugin": "相关插件已关闭。",
  "condition.remedy.project.init": "在此设置 PerMark",
  "condition.remedy.agent.turnOn": "打开代理连接",
  "condition.remedy.plugins.enable": "打开插件",
  "condition.remedy.settings.git": "git 设置",
} satisfies Messages;
