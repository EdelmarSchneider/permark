// This window's condition context: pushed by the backend (`context-changed` for this window), read by the UI.

import type { Context } from "../../api";
import type { Platform } from "../../api/generated/Platform";
import { available, type Availability, type Requirement } from "./available";

/** What a web view tells about the system it runs on (`userAgentData`: Chromium's web views only). */
type HostNavigator = { platform?: string; userAgentData?: { platform?: string } };

/** The system this window runs on, as its web view tells it; Windows when it does not say. Used until the backend's
 *  context arrives, so shortcuts and key labels fit the system from the first frame. */
export function hostPlatform(nav: HostNavigator | undefined = globalThis.navigator): Platform {
  // an empty answer says nothing: the next one is asked
  const name = nav?.userAgentData?.platform || nav?.platform || "";
  if (/^(mac|iphone|ipad)/i.test(name)) return "mac";
  if (/linux|bsd|x11/i.test(name)) return "linux";
  return "windows";
}

export function neutralContext(): Context {
  return {
    workspace: "welcome",
    git: { status: "noRepo" },
    permark: "notInitialized",
    agent: { status: "off" },
    liveUpdates: { status: "unavailable", reason: "noWorkspace" },
    distribution: "direct",
    platform: hostPlatform(),
    plugins: [],
    agentFiles: [],
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
