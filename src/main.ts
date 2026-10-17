// first: the choices remembered under the old name come over before any store reads them
import "./lib/legacyStorageBoot";
import { mount } from "svelte";
import "./styles/tokens.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/markdown.css";
import "./styles/tags.css";
import "./styles/filter.css";
import "./styles/switcher.css";
import "./styles/everywhere.css";
import "./styles/outline.css";
import "./styles/editor.css";
import "./styles/fileops.css";
import "./styles/visual.css";
import "./styles/unread.css";
import "./styles/agent.css";
import "./styles/comments.css";
import "./styles/notes.css";
import "./styles/media.css";
import App from "./App.svelte";
import { loadLanguage } from "./lib/stores/languageChoice.svelte";
import { loadTone, showCachedTone } from "./lib/stores/toneChoice.svelte";

// resolve the theme before the first paint so a dark system does not flash white
document.documentElement.dataset.theme = window.matchMedia("(prefers-color-scheme: dark)").matches
  ? "dark"
  : "light";

// the tone of the light theme the same way: the one shown last time is painted at once, and the stored choice (the
// backend's) is confirmed below, so a soft tone does not start with a flash of plain white
showCachedTone();

// the language too: the first paint is already in the person's language
await Promise.all([loadLanguage(), loadTone()]);

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
