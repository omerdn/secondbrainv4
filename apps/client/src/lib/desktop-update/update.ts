/**
 * Desktop self-update (T4.1.4, ADR-0023): on launch the shell asks GitHub Releases for a newer
 * signed build, downloads and installs it, then restarts, like Discord. Pure flow with the
 * Tauri calls injected, so it is tested without the shell.
 */

/** Progress events of `Update.downloadAndInstall` (`@tauri-apps/plugin-updater`). */
export type DownloadEvent =
  | { event: "Started"; data: { contentLength?: number } }
  | { event: "Progress"; data: { chunkLength: number } }
  | { event: "Finished" };

/** The parts of the plugin's `Update` used here. */
export interface AvailableUpdate {
  version: string;
  downloadAndInstall(onEvent?: (event: DownloadEvent) => void): Promise<void>;
}

export interface UpdaterDeps {
  /** Resolves to the update, or `null` when the app is current. */
  check(): Promise<AvailableUpdate | null>;
  relaunch(): Promise<void>;
}

export type UpdatePhase =
  | { kind: "idle" }
  | { kind: "downloading"; version: string; percent: number | null }
  | { kind: "restarting"; version: string };

/**
 * Checks, downloads, installs and restarts. Any failure (offline, GitHub down, bad signature)
 * leaves the current version running: updating must never block the app.
 */
export async function runDesktopUpdate(
  deps: UpdaterDeps,
  onPhase: (phase: UpdatePhase) => void,
): Promise<"current" | "updated" | "failed"> {
  let update: AvailableUpdate | null;
  try {
    update = await deps.check();
  } catch {
    return "failed";
  }
  if (update === null) return "current";

  const { version } = update;
  let total: number | null = null;
  let received = 0;
  onPhase({ kind: "downloading", version, percent: null });
  try {
    await update.downloadAndInstall((event) => {
      if (event.event === "Started") {
        total = event.data.contentLength ?? null;
      } else if (event.event === "Progress") {
        received += event.data.chunkLength;
        const percent = total ? Math.min(100, Math.round((received / total) * 100)) : null;
        onPhase({ kind: "downloading", version, percent });
      }
    });
  } catch {
    onPhase({ kind: "idle" });
    return "failed";
  }
  onPhase({ kind: "restarting", version });
  await deps.relaunch();
  return "updated";
}
