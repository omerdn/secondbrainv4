import { useBreakpoint } from "@secondbrain/ui";
import type { ReactNode } from "react";
import { View } from "react-native";

import { Sidebar } from "./Sidebar";
import { TabBar } from "./TabBar";

/**
 * Frame of every signed-in screen (T0.4.6): a sidebar from `md` on, bottom tabs below it. The
 * same component tree serves web, desktop and mobile (ADR-0012); only the breakpoint decides.
 */
export function AppShell({ account, children }: { account: string | null; children: ReactNode }) {
  const compact = useBreakpoint() === "compact";
  return (
    <View className={compact ? "flex-1 bg-background" : "flex-1 flex-row bg-background"}>
      {compact ? null : <Sidebar account={account} />}
      <View className="flex-1">{children}</View>
      {compact ? <TabBar /> : null}
    </View>
  );
}
