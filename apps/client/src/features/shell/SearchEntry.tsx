import Ionicons from "@expo/vector-icons/Ionicons";
import { cx, Text, themes, useScheme } from "@secondbrain/ui";
import { useTranslation } from "react-i18next";
import { Platform, Pressable, View } from "react-native";

/** The touchable way into global search (T2.7.6); on web it also shows the shortcut. */
export function SearchEntry({
  onPress,
  variant,
}: {
  onPress: () => void;
  variant: "sidebar" | "bar";
}) {
  const { t } = useTranslation();
  const colors = themes[useScheme()];
  return (
    <Pressable
      accessibilityRole="button"
      accessibilityLabel={t("search.open")}
      onPress={onPress}
      className={cx(
        "flex-row items-center gap-3 rounded-md border border-border px-3 py-2 active:opacity-70",
        variant === "bar" && "bg-surface",
      )}
    >
      <Ionicons name="search-outline" size={18} color={colors.muted} />
      <View className="flex-1">
        <Text tone="muted" numberOfLines={1}>
          {t("search.open")}
        </Text>
      </View>
      {Platform.OS === "web" && variant === "sidebar" ? (
        <Text variant="caption" tone="muted">
          {t("search.shortcut")}
        </Text>
      ) : null}
    </Pressable>
  );
}
