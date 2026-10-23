// Build check: no mock backend module (src/api/mock*.ts) or mock fixture (src/api/generated/mock-*.json) in a build
// other than `--mode mock`. The mock + its fixtures are ~200 KB that would load at every start (perf F2, 0.6.3).

import type { Plugin } from "vite";

const MOCK_MODULE = /\/src\/api\/(mock[^/]*\.ts|generated\/mock-[^/]*\.json)$/;

/** True for a mock backend module or fixture; `id` = bundler module id (any separator, may carry `?query`). */
export function isMockModule(id: string): boolean {
  const path = id.split("?")[0]?.replaceAll("\\", "/") ?? "";
  return MOCK_MODULE.test(path);
}

type BundleItem =
  { type: "chunk"; fileName: string; moduleIds: readonly string[] } | { type: "asset" };

/** `<chunk file>: <module id>` for every mock module found in the bundle's chunks. */
export function mockModulesIn(bundle: Record<string, BundleItem>): string[] {
  const found: string[] = [];
  for (const item of Object.values(bundle)) {
    if (item.type !== "chunk") continue;
    for (const id of item.moduleIds) if (isMockModule(id)) found.push(`${item.fileName}: ${id}`);
  }
  return found;
}

/** Fails `vite build` (any mode but `mock`) when a mock module reaches the output. */
export function noMockInProduction(): Plugin {
  return {
    name: "no-mock-in-production",
    apply: (_config, env) => env.command === "build" && env.mode !== "mock",
    generateBundle(_options, bundle) {
      const found = mockModulesIn(bundle as Record<string, BundleItem>);
      if (found.length > 0) {
        this.error(
          `mock modules in a production build (import them only behind import.meta.env.MODE === "mock"):\n${found.join("\n")}`,
        );
      }
    },
  };
}
