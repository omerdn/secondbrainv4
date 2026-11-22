import { act, render, screen } from "@testing-library/react-native";
import { I18nextProvider } from "react-i18next";

import { DesktopUpdater, runDesktopUpdate, type UpdatePhase } from "@/lib/desktop-update";
import type { AvailableUpdate, DownloadEvent } from "@/lib/desktop-update/update";
import { i18n } from "@/lib/i18n";

// T4.1.4 / ADR-0023: the desktop shell updates itself on launch, like Discord.
function update(events: DownloadEvent[], fail = false): AvailableUpdate {
  return {
    version: "0.2.0",
    downloadAndInstall: (onEvent) => {
      for (const event of events) onEvent?.(event);
      return fail ? Promise.reject(new Error("bad signature")) : Promise.resolve();
    },
  };
}

describe("runDesktopUpdate", () => {
  it("downloads with progress, installs and restarts", async () => {
    const phases: UpdatePhase[] = [];
    const relaunch = jest.fn(() => Promise.resolve());
    const result = await runDesktopUpdate(
      {
        check: () =>
          Promise.resolve(
            update([
              { event: "Started", data: { contentLength: 200 } },
              { event: "Progress", data: { chunkLength: 50 } },
              { event: "Progress", data: { chunkLength: 150 } },
              { event: "Finished" },
            ]),
          ),
        relaunch,
      },
      (p) => phases.push(p),
    );
    expect(result).toBe("updated");
    expect(phases).toEqual([
      { kind: "downloading", version: "0.2.0", percent: null },
      { kind: "downloading", version: "0.2.0", percent: 25 },
      { kind: "downloading", version: "0.2.0", percent: 100 },
      { kind: "restarting", version: "0.2.0" },
    ]);
    expect(relaunch).toHaveBeenCalledTimes(1);
  });

  it("does nothing when the app is current", async () => {
    const onPhase = jest.fn();
    const result = await runDesktopUpdate(
      { check: () => Promise.resolve(null), relaunch: jest.fn() },
      onPhase,
    );
    expect(result).toBe("current");
    expect(onPhase).not.toHaveBeenCalled();
  });

  it("keeps the running version when the check or the install fails", async () => {
    const relaunch = jest.fn(() => Promise.resolve());
    expect(
      await runDesktopUpdate(
        { check: () => Promise.reject(new Error("offline")), relaunch },
        jest.fn(),
      ),
    ).toBe("failed");
    const phases: UpdatePhase[] = [];
    expect(
      await runDesktopUpdate({ check: () => Promise.resolve(update([], true)), relaunch }, (p) =>
        phases.push(p),
      ),
    ).toBe("failed");
    expect(phases.at(-1)).toEqual({ kind: "idle" });
    expect(relaunch).not.toHaveBeenCalled();
  });
});

describe("DesktopUpdater", () => {
  const renderUpdater = (updater: Parameters<typeof DesktopUpdater>[0]["updater"]) =>
    render(
      <I18nextProvider i18n={i18n}>
        <DesktopUpdater updater={updater} />
      </I18nextProvider>,
    );

  it("renders nothing outside the desktop shell", async () => {
    await renderUpdater(null);
    expect(screen.toJSON()).toBeNull();
  });

  it("covers the app while an update downloads", async () => {
    let finish!: () => void;
    const pending: AvailableUpdate = {
      version: "0.2.0",
      downloadAndInstall: () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    };
    const relaunch = jest.fn(() => new Promise<void>(() => undefined));
    await renderUpdater({ check: () => Promise.resolve(pending), relaunch });
    expect(
      await screen.findByText(i18n.t("desktopUpdate.downloading", { version: "0.2.0" })),
    ).toBeOnTheScreen();
    await act(() => {
      finish();
      return Promise.resolve();
    });
    expect(await screen.findByText(i18n.t("desktopUpdate.restarting"))).toBeOnTheScreen();
    expect(relaunch).toHaveBeenCalled();
  });
});
