import "../global.css";

import { ApiProvider } from "@secondbrain/api-client/react";
import { ThemeProvider, ToastProvider, useScheme } from "@secondbrain/ui";
import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { SafeAreaProvider } from "react-native-safe-area-context";

import { SessionProvider } from "@/features/auth";
import { api, idempotencyKey, queryClient } from "@/lib/api";

function ThemedStatusBar() {
  return <StatusBar style={useScheme() === "dark" ? "light" : "dark"} />;
}

/** Root layout: API + query client, session (T0.4.5), theme (ADR-0017), one stack for every platform (ADR-0012). */
export default function RootLayout() {
  return (
    <SafeAreaProvider>
      <ApiProvider api={api} queryClient={queryClient} idempotencyKey={idempotencyKey}>
        <SessionProvider>
          <ThemeProvider>
            <ToastProvider>
              <ThemedStatusBar />
              <Stack screenOptions={{ headerShown: false }} />
            </ToastProvider>
          </ThemeProvider>
        </SessionProvider>
      </ApiProvider>
    </SafeAreaProvider>
  );
}
