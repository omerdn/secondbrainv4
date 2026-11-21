import "../global.css";

import { ThemeProvider, ToastProvider, useScheme } from "@secondbrain/ui";
import { Stack } from "expo-router";
import { StatusBar } from "expo-status-bar";
import { SafeAreaProvider } from "react-native-safe-area-context";

function ThemedStatusBar() {
  return <StatusBar style={useScheme() === "dark" ? "light" : "dark"} />;
}

/** Root layout: theme (ADR-0017) and one stack for every platform (ADR-0012). */
export default function RootLayout() {
  return (
    <SafeAreaProvider>
      <ThemeProvider>
        <ToastProvider>
          <ThemedStatusBar />
          <Stack screenOptions={{ headerShown: false }} />
        </ToastProvider>
      </ThemeProvider>
    </SafeAreaProvider>
  );
}
