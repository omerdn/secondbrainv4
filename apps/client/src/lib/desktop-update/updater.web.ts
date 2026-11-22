import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";

import type { UpdaterDeps } from "./update";

/** The shell's updater and process plugins (web build only; used inside Tauri). */
export const desktopUpdater: UpdaterDeps | null = { check, relaunch };
