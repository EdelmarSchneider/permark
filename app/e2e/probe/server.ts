// The app in mock mode (`vite --mode mock`) on a free loopback port, for one probe run (spec: probe.md §4). Started and
// stopped by the runner: whatever starts here is stopped in `stop` (teardown symmetry).

import { spawn, spawnSync, type ChildProcess } from "node:child_process";
import { existsSync } from "node:fs";
import { createServer } from "node:net";
import path from "node:path";

export type MockServer = { url: string; stop(): void };

const READY_MS = 60_000;

function freePort(): Promise<number> {
  return new Promise((resolve, reject) => {
    const srv = createServer();
    srv.once("error", reject);
    srv.listen(0, "127.0.0.1", () => {
      const address = srv.address();
      const port = typeof address === "object" && address !== null ? address.port : 0;
      srv.close(() => (port > 0 ? resolve(port) : reject(new Error("no free port"))));
    });
  });
}

function kill(child: ChildProcess): void {
  if (child.pid === undefined || child.exitCode !== null) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/pid", String(child.pid), "/T", "/F"], { stdio: "ignore" });
  } else {
    child.kill("SIGTERM");
  }
}

async function ready(url: string, child: ChildProcess, log: () => string): Promise<void> {
  const until = Date.now() + READY_MS;
  while (Date.now() < until) {
    if (child.exitCode !== null)
      throw new Error(`mock server exited (${child.exitCode}):\n${log()}`);
    try {
      const res = await fetch(url);
      if (res.ok) return;
    } catch {
      // not listening yet
    }
    await new Promise((r) => setTimeout(r, 250));
  }
  throw new Error(`mock server not ready in ${READY_MS / 1000} s:\n${log()}`);
}

/** Serves `appDir` (an app folder with its own node_modules) in mock mode. */
export async function startMock(appDir: string): Promise<MockServer> {
  const vite = path.join(appDir, "node_modules", "vite", "bin", "vite.js");
  if (!existsSync(vite)) throw new Error(`no vite in ${appDir} (npm ci there first)`);
  const port = await freePort();
  const child = spawn(
    process.execPath,
    [vite, "--mode", "mock", "--host", "127.0.0.1", "--port", String(port), "--strictPort"],
    { cwd: appDir, stdio: ["ignore", "pipe", "pipe"], env: { ...process.env, BROWSER: "none" } },
  );
  let output = "";
  const keep = (b: Buffer): void => {
    output = (output + b.toString()).slice(-4000);
  };
  child.stdout?.on("data", keep);
  child.stderr?.on("data", keep);
  const url = `http://127.0.0.1:${port}/`;
  const stop = (): void => kill(child);
  try {
    await ready(url, child, () => output);
  } catch (e) {
    stop();
    throw e;
  }
  return { url, stop };
}
