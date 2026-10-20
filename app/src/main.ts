import "./core/styles/fonts.css";
import "./core/styles/tokens.css";
import "./core/styles/base.css";
import "./core/styles/controls.css";
import { mount } from "svelte";
import App from "./App.svelte";
import { api } from "./api";
import { look } from "./core/look.svelte";
import { windowStore } from "./core/window.svelte";

// Window starts hidden. System look first (in case boot fails), then the person's settings + what this window shows,
// all before first paint (no flash, no English flash), then mount, then show.
look.apply();
await windowStore.start();

try {
  const target = document.getElementById("app");
  if (target) mount(App, { target });
} finally {
  // Shown even if mounting failed: a hidden window would look like a dead app.
  requestAnimationFrame(() => {
    void api.showWindow();
  });
}
