import "../global.css";

import { ApiProvider } from "@secondbrain/api-client/react";
import { ThemeProvider, ToastProvider, useScheme } from "@secondbrain/ui";
import { Stack } from "expo-router";
import type { ReactNode } from "react";
import { StatusBar } from "expo-status-bar";
import { I18nextProvider } from "react-i18next";
import { GestureHandlerRootView } from "react-native-gesture-handler";
import { SafeAreaProvider } from "react-native-safe-area-context";

import { ConnectivityGate, ConnectivityProvider, useOnConnected } from "@/connectivity";
import { LanguageSync, SessionProvider, useSession } from "@/features/auth";
import { api, idempotencyKey, queryClient } from "@/lib/api";
import { i18n } from "@/lib/i18n";

/**
 * A session restore that failed for lack of connection is retried when the connection comes
 * up (only then, so a server answering 5xx is not retried in a loop).
 */
function SessionRecovery() {
  const { state, retry } = useSession();
  useOnConnected(() => {
    if (state.status === "unavailable") retry();
  });
  return null;
}

/** Connection state machine (T0.5.3); the session decides between `/ws` and the probe. */
function Connectivity({ children }: { children: ReactNode }) {
  const { state, forget } = useSession();
  return (
    <ConnectivityProvider
      userId={state.status === "signedIn" ? state.user.id : null}
      // Signed out elsewhere (or the session expired): back to the login screen (T0.5.7).
      onSessionEnded={() => void forget()}
    >
      <SessionRecovery />
      {children}
    </ConnectivityProvider>
  );
}

function ThemedStatusBar() {
  return <StatusBar style={useScheme() === "dark" ? "light" : "dark"} />;
}

/** Root layout: API + query client, session (T0.4.5), connectivity (T0.5.3) and its screens (T0.5.4), i18n (T0.4.7), theme (ADR-0017), one stack for every platform (ADR-0012). */
export default function RootLayout() {
  return (
    <GestureHandlerRootView style={{ flex: 1 }}>
      <SafeAreaProvider>
        <I18nextProvider i18n={i18n}>
          <ApiProvider api={api} queryClient={queryClient} idempotencyKey={idempotencyKey}>
            <SessionProvider>
              <LanguageSync />
              <Connectivity>
                <ThemeProvider>
                  <ToastProvider>
                    <ThemedStatusBar />
                    <ConnectivityGate>
                      <Stack screenOptions={{ headerShown: false }} />
                    </ConnectivityGate>
                  </ToastProvider>
                </ThemeProvider>
              </Connectivity>
            </SessionProvider>
          </ApiProvider>
        </I18nextProvider>
      </SafeAreaProvider>
    </GestureHandlerRootView>
  );
}
