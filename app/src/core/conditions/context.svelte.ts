// This window's condition context: pushed by the backend (`context-changed` for this window), read by the UI.

import type { Context } from "../../api";
import { available, type Availability, type Requirement } from "./available";

export function neutralContext(): Context {
  return {
    workspace: "welcome",
    git: { status: "noRepo" },
    permark: "notInitialized",
    agent: { status: "off" },
    liveUpdates: { status: "unavailable", reason: "noWorkspace" },
    distribution: "direct",
    platform: "windows",
    plugins: [],
  };
}

class ContextStore {
  current = $state.raw<Context>(neutralContext());

  set(context: Context): void {
    this.current = context;
  }

  available(requirement: Requirement): Availability {
    return available(requirement, this.current);
  }
}

export const conditions = new ContextStore();
