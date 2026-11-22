import { Button, Text } from "@secondbrain/ui";
import { type ReactNode, useEffect } from "react";
import { useTranslation } from "react-i18next";
import { ActivityIndicator, Platform, View } from "react-native";

import { useConnectivity } from "./ConnectivityProvider";

function FullScreen({ children, testID }: { children: ReactNode; testID: string }) {
  return (
    <View testID={testID} className="flex-1 items-center justify-center gap-4 bg-background px-6">
      {children}
    </View>
  );
}

/**
 * Shows the app only when connected (PRD BP1–BP3, ARCHITECTURE §10.2):
 * - `booting`: a loader; nothing from the app, not even cached data (BP1).
 * - `offline_blocked`: "no connection" with a retry button (BP1).
 * - `reconnecting`: the app stays mounted under a full-screen lock, so forms keep what was
 *   typed and in-flight requests still end in a confirmation or a "could not save" error
 *   (BP2, BP3). Touches, and on web keyboard focus, cannot reach it.
 */
export function ConnectivityGate({ children }: { children: ReactNode }) {
  const { t } = useTranslation();
  const { state, retry } = useConnectivity();
  const locked = state.kind === "reconnecting";

  // Web: a focused field under the lock would still take keystrokes.
  useEffect(() => {
    if (locked && Platform.OS === "web" && typeof document !== "undefined") {
      const active = document.activeElement;
      if (active instanceof HTMLElement) active.blur();
    }
  }, [locked]);

  switch (state.kind) {
    case "booting":
      return (
        <FullScreen testID="connectivity-booting">
          <ActivityIndicator accessibilityLabel={t("connectivity.connecting")} />
        </FullScreen>
      );
    case "offline_blocked":
      return (
        <FullScreen testID="connectivity-offline">
          <Text variant="heading" className="text-center">
            {t("connectivity.offline.title")}
          </Text>
          <Text tone="muted" className="max-w-sm text-center">
            {t("connectivity.offline.description")}
          </Text>
          <Button title={t("connectivity.offline.retry")} onPress={retry} />
        </FullScreen>
      );
    case "online":
    case "reconnecting":
      return (
        <View className="flex-1">
          <View
            className="flex-1"
            // Hidden from screen readers while locked; the lock announces itself instead.
            aria-hidden={locked}
            importantForAccessibility={locked ? "no-hide-descendants" : "auto"}
          >
            {children}
          </View>
          {locked ? (
            <View
              testID="connectivity-lock"
              accessibilityViewIsModal
              accessibilityLiveRegion="polite"
              aria-modal
              role="alert"
              className="absolute inset-0 items-center justify-center gap-4 bg-background/90 px-6"
            >
              <ActivityIndicator />
              <Text variant="heading" className="text-center">
                {t("connectivity.reconnecting.title")}
              </Text>
              <Text tone="muted" className="max-w-sm text-center">
                {t("connectivity.reconnecting.description")}
              </Text>
            </View>
          ) : null}
        </View>
      );
  }
}
