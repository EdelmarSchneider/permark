// An editor with no headings, for the tests whose fake editors only need to satisfy the type (spread it into the fake).

import type { OutlineSource } from "../bridge";

export const noOutline: OutlineSource = {
  headings: () => [],
  scrollToHeading: () => {},
  currentHeading: () => null,
  onViewport: () => () => {},
};
