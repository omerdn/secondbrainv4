import { useBreakpoint } from "@secondbrain/ui";
import type { ReactNode } from "react";
import { View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { Sidebar } from "./Sidebar";
import { TabBar } from "./TabBar";

/**
 * Frame of every signed-in screen (T0.4.6): a sidebar from `md` on, bottom tabs below it. The
 * same component tree serves web, desktop and mobile (ADR-0012); only the breakpoint decides.
 */
export function AppShell({ account, children }: { account: string | null; children: ReactNode }) {
  const compact = useBreakpoint() === "compact";
  const insets = useSafeAreaInsets();
  return (
    <View className={compact ? "flex-1 bg-background" : "flex-1 flex-row bg-background"}>
      {compact ? null : <Sidebar account={account} />}
      {/* The top inset sits here so anything above a screen (banners) clears the notch too. */}
      <View className="flex-1" style={{ paddingTop: insets.top }}>
        {children}
      </View>
      {compact ? <TabBar /> : null}
    </View>
  );
}
