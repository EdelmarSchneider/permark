// The Explorer of the project this window shows, for the parts of the app that point at the tree (a document's header
// "reveal in tree", a link to a folder). Set by the sidebar while it lives.

type Revealer = { reveal(path: string): void; revealActive(): void };

let current: Revealer | null = null;

export function setExplorer(explorer: Revealer | null): void {
  current = explorer;
}

export function revealInTree(path: string | null): void {
  if (path === null) current?.revealActive();
  else current?.reveal(path);
}
