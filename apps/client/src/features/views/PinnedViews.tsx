import { useApi } from "@secondbrain/api-client/react";
import { cx, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useGlobalSearchParams, usePathname, useRouter } from "expo-router";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

/** Pinned saved views in the sidebar (PRD F2). Renders nothing when there are none. */
export function PinnedViews() {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const pathname = usePathname();
  const { view } = useGlobalSearchParams<{ view?: string }>();
  const views = useQuery({ queryKey: ["views"], queryFn: api.views.list });
  const pinned = views.data?.filter((entry) => entry.pinned) ?? [];
  if (pinned.length === 0) {
    return null;
  }
  return (
    <View className="gap-0.5 pt-4">
      <Text variant="caption" tone="muted" className="px-3 pb-1">
        {t("views.pinned")}
      </Text>
      {pinned.map((entry) => {
        const active = pathname === "/views" && view === entry.id;
        return (
          <Pressable
            key={entry.id}
            accessibilityRole="link"
            accessibilityLabel={entry.name}
            accessibilityState={{ selected: active }}
            className={cx("rounded-md px-3 py-2", active && "bg-surface-raised")}
            onPress={() => {
              const href = `/views?view=${encodeURIComponent(entry.id)}` as Href;
              router.push(href);
            }}
          >
            <Text variant="label" tone={active ? "primary" : "default"} numberOfLines={1}>
              {entry.name}
            </Text>
          </Pressable>
        );
      })}
    </View>
  );
}
