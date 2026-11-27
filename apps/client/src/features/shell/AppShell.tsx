import { useBreakpoint } from "@secondbrain/ui";
import type { ReactNode } from "react";
import { View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { type NavBadges, NavBadgesContext } from "./badges";
import { SearchEntry } from "./SearchEntry";
import { Sidebar } from "./Sidebar";
import { TabBar } from "./TabBar";

/**
 * Frame of every signed-in screen (T0.4.6): a sidebar from `md` on, bottom tabs below it. The
 * same component tree serves web, desktop and mobile (ADR-0012); only the breakpoint decides.
 */
export function AppShell({
  account,
  sidebarExtra,
  onSearch,
  badges = {},
  children,
}: {
  account: string | null;
  /** Shown in the sidebar under the sections (pinned saved views). */
  sidebarExtra?: ReactNode;
  /** Opens global search; its entry is in the sidebar, or above the screen on phones. */
  onSearch?: () => void;
  /** Counts on navigation entries (unread notifications). */
  badges?: NavBadges;
  children: ReactNode;
}) {
  const compact = useBreakpoint() === "compact";
  const insets = useSafeAreaInsets();
  return (
    <NavBadgesContext.Provider value={badges}>
      <View className={compact ? "flex-1 bg-background" : "flex-1 flex-row bg-background"}>
        {compact ? null : <Sidebar account={account} extra={sidebarExtra} onSearch={onSearch} />}
        {/* The top inset sits here so anything above a screen (banners) clears the notch too. */}
        <View className="flex-1" style={{ paddingTop: insets.top }}>
          {compact && onSearch !== undefined ? (
            <View className="px-4 pt-2">
              <SearchEntry onPress={onSearch} variant="bar" />
            </View>
          ) : null}
          {children}
        </View>
        {compact ? <TabBar /> : null}
      </View>
    </NavBadgesContext.Provider>
  );
}
