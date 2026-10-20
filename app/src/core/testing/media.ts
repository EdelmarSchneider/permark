// jsdom has no `matchMedia`: tests that load the window store import this first.

if (typeof window !== "undefined" && typeof window.matchMedia !== "function") {
  Object.defineProperty(window, "matchMedia", {
    value: () => ({
      matches: false,
      addEventListener: () => {},
      removeEventListener: () => {},
    }),
  });
}
if (typeof Element !== "undefined") Element.prototype.scrollIntoView = () => {};
