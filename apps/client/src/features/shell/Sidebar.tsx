import { layout, Text } from "@secondbrain/ui";
import { usePathname } from "expo-router";
import type { ReactNode } from "react";
import { ScrollView, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

import { APP_NAME } from "@/lib/app-info";

import { destinations, isActive, settings } from "./nav";
import { NavItem } from "./NavItem";
import { SearchEntry } from "./SearchEntry";

/** Left navigation from the `md` breakpoint on (web, desktop, tablets). */
export function Sidebar({
  account,
  extra,
  onSearch,
}: {
  account: string | null;
  extra?: ReactNode;
  onSearch?: () => void;
}) {
  const pathname = usePathname();
  const insets = useSafeAreaInsets();
  return (
    <View
      accessibilityRole="navigation"
      testID="sidebar"
      className="border-r border-border bg-surface"
      style={{ width: layout.sidebarWidth, paddingTop: insets.top, paddingBottom: insets.bottom }}
    >
      <View className="px-4 py-4">
        <Text variant="heading">{APP_NAME}</Text>
      </View>
      {onSearch === undefined ? null : (
        <View className="px-2 pb-2">
          <SearchEntry onPress={onSearch} variant="sidebar" />
        </View>
      )}
      <ScrollView contentContainerClassName="gap-0.5 px-2">
        {destinations.map((d) => (
          <NavItem
            key={d.key}
            destination={d}
            active={isActive(pathname, d.href)}
            variant="sidebar"
          />
        ))}
        {extra}
      </ScrollView>
      <View className="gap-1 border-t border-border px-2 py-2">
        {account !== null ? (
          <Text variant="caption" tone="muted" numberOfLines={1} className="px-3 pt-1">
            {account}
          </Text>
        ) : null}
        <NavItem
          destination={settings}
          active={isActive(pathname, settings.href)}
          variant="sidebar"
        />
      </View>
    </View>
  );
}
