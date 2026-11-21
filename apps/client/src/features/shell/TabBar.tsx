import { layout } from "@secondbrain/ui";
import { usePathname } from "expo-router";
import { View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { activeTab, tabs } from "./nav";
import { NavItem } from "./NavItem";

/** Bottom tabs below the `md` breakpoint (phones, narrow windows). */
export function TabBar() {
  const active = activeTab(usePathname());
  const insets = useSafeAreaInsets();
  return (
    <View
      accessibilityRole="tablist"
      testID="tab-bar"
      className="flex-row border-t border-border bg-surface"
      style={{ minHeight: layout.tabBarHeight + insets.bottom, paddingBottom: insets.bottom }}
    >
      {tabs.map((tab) => (
        <NavItem key={tab.key} destination={tab} active={active === tab.key} variant="tab" />
      ))}
    </View>
  );
}
