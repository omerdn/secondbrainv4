import Ionicons from "@expo/vector-icons/Ionicons";
import { cx, Text, themes, useScheme } from "@secondbrain/ui";
import { type Href, Link, useRouter } from "expo-router";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { useNavBadge } from "./badges";
import type { Destination } from "./nav";

/**
 * A navigation entry. Sidebar and list entries are links (on web a real `<a href>`: new tab,
 * middle click); bottom tabs are `tab`s that switch sections. Feedback is the pressed state
 * only, no hover (ADR-0012).
 */
export function NavItem({
  destination,
  active,
  variant,
}: {
  destination: Destination;
  active: boolean;
  variant: "sidebar" | "tab" | "list";
}) {
  const colors = themes[useScheme()];
  const { t } = useTranslation();
  const label = t(`nav.${destination.key}`);
  const badge = useNavBadge(destination.key);
  const tint = active ? colors.primary : colors.muted;
  const iconSize = variant === "tab" ? 24 : 20;
  const router = useRouter();
  const href = destination.href as Href;
  const item = (
    <Pressable
      accessibilityRole={variant === "tab" ? "tab" : "link"}
      onPress={variant === "tab" ? () => router.navigate(href) : undefined}
      accessibilityLabel={label}
      accessibilityState={{ selected: active }}
      aria-current={active ? "page" : undefined}
      className={cx(
        "active:opacity-70",
        variant === "tab"
          ? "flex-1 items-center justify-center gap-0.5 py-1.5"
          : "flex-row items-center gap-3 rounded-md px-3 py-2",
        variant === "sidebar" && active && "bg-surface-raised",
        variant === "list" && "py-3",
      )}
    >
      <Ionicons name={destination.icon} size={iconSize} color={tint} />
      <View className={variant === "tab" ? undefined : "flex-1"}>
        <Text
          variant={variant === "tab" ? "caption" : "body"}
          numberOfLines={1}
          className={cx(
            active ? "text-primary" : variant === "tab" ? "text-muted" : "text-foreground",
            active && "font-semibold",
          )}
        >
          {label}
        </Text>
      </View>
      {badge > 0 ? (
        <View
          testID={`nav-badge-${destination.key}`}
          className={cx(
            "min-w-5 items-center rounded-full bg-primary px-1.5",
            variant === "tab" && "absolute right-1/4 top-0.5",
          )}
        >
          <Text variant="caption" className="font-semibold text-white">
            {badge > 99 ? "99+" : String(badge)}
          </Text>
        </View>
      ) : null}
    </Pressable>
  );
  return variant === "tab" ? (
    item
  ) : (
    <Link href={href} asChild>
      {item}
    </Link>
  );
}
