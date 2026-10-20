import "./core/styles/fonts.css";
import "./core/styles/tokens.css";
import "./core/styles/base.css";
import "./core/styles/controls.css";
import { mount } from "svelte";
import App from "./App.svelte";
import { api } from "./api";
import { look } from "./core/look.svelte";
import { windowStore } from "./core/window.svelte";

// A new window starts hidden. System look first (in case boot fails), then the person's settings + what this view
// shows, all before first paint (no flash, no English flash), then mount, then tell the backend it painted (it shows
// the window with its view on screen).
look.apply();
await windowStore.start();

try {
  const target = document.getElementById("app");
  if (target) mount(App, { target });
} finally {
  // Shown even if mounting failed: a hidden window would look like a dead app. A view made hidden may get no frame:
  // the timer says it too (the backend takes the first).
  const painted = (): void => void api.showWindow();
  requestAnimationFrame(painted);
  setTimeout(painted, 200);
}
