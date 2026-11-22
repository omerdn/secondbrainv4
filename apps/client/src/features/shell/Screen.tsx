import Ionicons from "@expo/vector-icons/Ionicons";
import { EmptyState, layout, Text, themes, useScheme } from "@secondbrain/ui";
import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import { ScrollView, View } from "react-native";

import { destinations, type Section, settings } from "./nav";

/**
 * Page frame inside the shell: title and scrollable content, width-limited on wide screens.
 * `width="form"` keeps forms and settings in a narrower, readable column (T0.7.5).
 */
export function Screen({
  title,
  width = "content",
  children,
}: {
  title: string;
  width?: "content" | "form";
  children: ReactNode;
}) {
  const maxWidth = width === "form" ? layout.formMaxWidth : layout.contentMaxWidth;
  return (
    <ScrollView className="flex-1" contentContainerClassName="px-4 pb-8 md:px-8">
      <View className="w-full gap-4 self-center" style={{ maxWidth }}>
        <Text variant="title" className="pt-4 md:pt-8">
          {title}
        </Text>
        {children}
      </View>
    </ScrollView>
  );
}

/** Stand-in for a section that later tasks fill in: its icon and what it will hold. */
export function PlaceholderScreen({ section }: { section: Section }) {
  const { t } = useTranslation();
  const colors = themes[useScheme()];
  const icon = [...destinations, settings].find((d) => d.key === section)?.icon;
  return (
    <Screen title={t(`nav.${section}`)}>
      <EmptyState
        icon={
          icon !== undefined ? (
            <Ionicons name={icon} size={40} color={colors.muted} accessibilityElementsHidden />
          ) : undefined
        }
        title={t("common.comingSoon.title")}
        description={t(`common.comingSoon.sections.${section}`)}
      />
    </Screen>
  );
}
