import { getVersion } from "@tauri-apps/api/app";

import { isTauri } from "@/lib/platform";

import type { ClientPlatform } from "./current";

/** Desktop shell: the app version from Tauri. A plain browser always runs the current web app. */
export async function currentClient(): Promise<{
  platform: ClientPlatform;
  version: string;
} | null> {
  if (!isTauri()) return null;
  return { platform: "desktop", version: await getVersion() };
}
