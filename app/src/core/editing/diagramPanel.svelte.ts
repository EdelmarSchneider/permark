// The optional diagram preview panel beside the Source editor: off by default, one switch per window (kept while the
// window lives).

class DiagramPanel {
  on = $state(false);

  toggle(): void {
    this.on = !this.on;
  }
}

export const diagramPanel = new DiagramPanel();
