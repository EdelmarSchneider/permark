// The registry the core UI renders contributions from (one per window). Plugins register into it through the host.

import { ContributionRegistry } from "./registry.svelte";

export const contributions = new ContributionRegistry();
