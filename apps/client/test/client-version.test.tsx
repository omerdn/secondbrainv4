import type { ClientPolicy } from "@secondbrain/api-types";
import { act, fireEvent, render, screen } from "@testing-library/react-native";
import { I18nextProvider } from "react-i18next";

import { isOutdated, parseVersion, UpdateRequired } from "@/lib/client-version";
import { i18n } from "@/lib/i18n";

// T4.2.7 / ADR-0023: apps older than the server's minimum version must update.
describe("version comparison", () => {
  it("parses major.minor.patch only", () => {
    expect(parseVersion("1.10.3")).toEqual([1, 10, 3]);
    expect(parseVersion("1.2")).toBeNull();
    expect(parseVersion("1.2.3-beta")).toBeNull();
  });

  it("compares numerically and never blocks on unknown versions", () => {
    expect(isOutdated("0.1.0", "0.2.0")).toBe(true);
    expect(isOutdated("0.9.9", "0.10.0")).toBe(true);
    expect(isOutdated("0.10.0", "0.9.9")).toBe(false);
    expect(isOutdated("0.2.0", "0.2.0")).toBe(false);
    expect(isOutdated("1.0.0", null)).toBe(false);
    expect(isOutdated(null, "1.0.0")).toBe(false);
    expect(isOutdated("dev", "1.0.0")).toBe(false);
  });
});

const policy = (min: string | null): ClientPolicy => ({
  ios: { min_version: null, download_url: null },
  android: { min_version: null, download_url: null },
  desktop: { min_version: min, download_url: "https://example.test/desktop" },
});

const renderGate = (deps: Parameters<typeof UpdateRequired>[0]["deps"]) =>
  render(
    <I18nextProvider i18n={i18n}>
      <UpdateRequired deps={deps} />
    </I18nextProvider>,
  );

describe("UpdateRequired", () => {
  it("blocks an outdated desktop app and opens the download page", async () => {
    const open = jest.fn(() => Promise.resolve());
    await renderGate({
      current: () => Promise.resolve({ platform: "desktop", version: "0.1.0" }),
      policy: () => Promise.resolve(policy("0.2.0")),
      open,
    });
    expect(await screen.findByText(i18n.t("updateRequired.title"))).toBeOnTheScreen();
    await act(() => fireEvent.press(screen.getByRole("button")));
    expect(open).toHaveBeenCalledWith("https://example.test/desktop");
  });

  it("lets current apps, browsers and failed checks through", async () => {
    for (const deps of [
      {
        current: () => Promise.resolve({ platform: "desktop" as const, version: "0.2.0" }),
        policy: () => Promise.resolve(policy("0.2.0")),
      },
      { current: () => Promise.resolve(null), policy: () => Promise.resolve(policy("9.0.0")) },
      {
        current: () => Promise.resolve({ platform: "desktop" as const, version: "0.1.0" }),
        policy: () => Promise.reject(new Error("offline")),
      },
    ]) {
      const view = await renderGate({ ...deps, open: jest.fn() });
      await act(() => Promise.resolve());
      expect(screen.queryByText(i18n.t("updateRequired.title"))).toBeNull();
      await view.unmount();
    }
  });
});
