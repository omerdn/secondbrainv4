import { renderRouter, screen } from "expo-router/testing-library";
import { act } from "react";
import { Text } from "react-native";

import { DesktopDeepLinks, deepLinkPath } from "@/lib/desktop-links";

// T4.1.5: routune:// links open the desktop app on the matching screen.
describe("deepLinkPath", () => {
  it("maps routune links to in-app routes", () => {
    expect(deepLinkPath("routune://verify-email?token=abc")).toBe("/verify-email?token=abc");
    expect(deepLinkPath("routune://reset-password/?token=x%2By")).toBe(
      "/reset-password?token=x%2By",
    );
    expect(deepLinkPath("routune://tasks/42")).toBe("/tasks/42");
    expect(deepLinkPath("routune://")).toBe("/");
  });

  it("ignores other schemes, traversal and malformed links", () => {
    expect(deepLinkPath("https://evil.example/settings")).toBeNull();
    expect(deepLinkPath("javascript:alert(1)")).toBeNull();
    expect(deepLinkPath("not a url")).toBeNull();
    expect(deepLinkPath("routune://tasks/../../etc")).toBe("/tasks/etc");
  });
});

describe("DesktopDeepLinks", () => {
  it("navigates when a link arrives", async () => {
    let deliver: ((url: string) => void) | null = null;
    const listen = (onLink: (url: string) => void) => {
      deliver = onLink;
      return () => undefined;
    };
    await renderRouter(
      {
        _layout: () => <DesktopDeepLinks listen={listen} />,
        index: () => <Text>home</Text>,
        settings: () => <Text>settings screen</Text>,
      },
      { initialUrl: "/" },
    );
    await act(() => {
      deliver?.("routune://settings");
      return Promise.resolve();
    });
    expect(screen).toHavePathname("/settings");
  });
});
