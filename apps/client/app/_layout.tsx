import "../global.css";

import { ApiProvider } from "@secondbrain/api-client/react";
import { ThemeProvider, ToastProvider, useScheme } from "@secondbrain/ui";
import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { I18nextProvider } from "react-i18next";
import { SafeAreaProvider } from "react-native-safe-area-context";

import { LanguageSync, SessionProvider } from "@/features/auth";
import { api, idempotencyKey, queryClient } from "@/lib/api";
import { i18n } from "@/lib/i18n";

function ThemedStatusBar() {
  return <StatusBar style={useScheme() === "dark" ? "light" : "dark"} />;
}

/** Root layout: API + query client, session (T0.4.5), i18n (T0.4.7), theme (ADR-0017), one stack for every platform (ADR-0012). */
export default function RootLayout() {
  return (
    <SafeAreaProvider>
      <I18nextProvider i18n={i18n}>
        <ApiProvider api={api} queryClient={queryClient} idempotencyKey={idempotencyKey}>
          <SessionProvider>
            <LanguageSync />
            <ThemeProvider>
              <ToastProvider>
                <ThemedStatusBar />
                <Stack screenOptions={{ headerShown: false }} />
              </ToastProvider>
            </ThemeProvider>
          </SessionProvider>
        </ApiProvider>
      </I18nextProvider>
    </SafeAreaProvider>
  );
}
