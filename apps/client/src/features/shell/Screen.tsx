import { EmptyState, layout, Text } from "@secondbrain/ui";
import type { ReactNode } from "react";
import { ScrollView, View } from "react-native";
import { useSafeAreaInsets } from "react-native-safe-area-context";

/** Page frame inside the shell: title and scrollable content, width-limited on wide screens. */
export function Screen({ title, children }: { title: string; children: ReactNode }) {
  const insets = useSafeAreaInsets();
  return (
    <ScrollView
      className="flex-1"
      contentContainerClassName="px-4 pb-8 md:px-8"
      contentContainerStyle={{ paddingTop: insets.top }}
    >
      <View className="w-full gap-4 self-center" style={{ maxWidth: layout.contentMaxWidth }}>
        <Text variant="title" className="pt-4 md:pt-8">
          {title}
        </Text>
        {children}
      </View>
    </ScrollView>
  );
}

/** Stand-in for a section that later tasks fill in. */
export function PlaceholderScreen({ title }: { title: string }) {
  return (
    <Screen title={title}>
      <EmptyState title="Yakında" description="Bu bölüm sonraki sürümlerde gelecek." />
    </Screen>
  );
}
