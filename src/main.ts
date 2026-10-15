import { mount } from "svelte";
import "./styles/tokens.css";
import "./styles/layout.css";
import "./styles/components.css";
import "./styles/markdown.css";
import "./styles/tags.css";
import "./styles/filter.css";
import "./styles/everywhere.css";
import "./styles/outline.css";
import "./styles/editor.css";
import "./styles/fileops.css";
import "./styles/visual.css";
import "./styles/unread.css";
import "./styles/agent.css";
import App from "./App.svelte";
import { loadLanguage } from "./lib/stores/languageChoice.svelte";

// resolve the theme before the first paint so a dark system does not flash white
document.documentElement.dataset.theme = window.matchMedia("(prefers-color-scheme: dark)").matches
  ? "dark"
  : "light";

// the language too: the first paint is already in the person's language
await loadLanguage();

const app = mount(App, {
  target: document.getElementById("app")!,
});

export default app;
