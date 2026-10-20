// Whether the window is in front and visible: what a person can be looking at (reading counts only then).

export class InFront {
  value = $state(true);

  /** Follows the window until the returned function runs; call it from an effect: `$effect(() => front.start())`. */
  start(): () => void {
    const sync = (): void => {
      this.value = document.visibilityState === "visible" && document.hasFocus();
    };
    sync();
    window.addEventListener("focus", sync);
    window.addEventListener("blur", sync);
    document.addEventListener("visibilitychange", sync);
    return () => {
      window.removeEventListener("focus", sync);
      window.removeEventListener("blur", sync);
      document.removeEventListener("visibilitychange", sync);
    };
  }
}
