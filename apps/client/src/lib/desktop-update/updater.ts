import type { UpdaterDeps } from "./update";

/** Native builds update through the stores / OTA (ADR-0023), not the desktop updater. */
export const desktopUpdater: UpdaterDeps | null = null;
