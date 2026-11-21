import "../global.css";

import { ApiProvider } from "@secondbrain/api-client/react";
import { ThemeProvider, ToastProvider, useScheme } from "@secondbrain/ui";
import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { SafeAreaProvider } from "react-native-safe-area-context";

import { api, idempotencyKey, queryClient } from "@/lib/api";

function ThemedStatusBar() {
  return <StatusBar style={useScheme() === "dark" ? "light" : "dark"} />;
}

/** Root layout: API + query client, theme (ADR-0017), one stack for every platform (ADR-0012). */
export default function RootLayout() {
  return (
    <SafeAreaProvider>
      <ApiProvider api={api} queryClient={queryClient} idempotencyKey={idempotencyKey}>
        <ThemeProvider>
          <ToastProvider>
            <ThemedStatusBar />
            <Stack screenOptions={{ headerShown: false }} />
          </ToastProvider>
        </ThemeProvider>
      </ApiProvider>
    </SafeAreaProvider>
  );
}
