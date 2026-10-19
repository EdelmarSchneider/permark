import "./core/styles/fonts.css";
import "./core/styles/tokens.css";
import "./core/styles/base.css";
import { mount } from "svelte";
import App from "./App.svelte";
import { api } from "./api";
import { language } from "./core/i18n";
import { applySystemTheme } from "./core/theme";

// Window starts hidden: theme + language resolved before first paint, then shown (no flash).
applySystemTheme();
await language.choose("system");

try {
  const target = document.getElementById("app");
  if (target) mount(App, { target });
} finally {
  // Shown even if mounting failed: a hidden window would look like a dead app.
  requestAnimationFrame(() => {
    void api.showWindow();
  });
}
