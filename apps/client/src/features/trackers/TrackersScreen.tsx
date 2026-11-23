import { useApi } from "@secondbrain/api-client/react";
import { Button, EmptyState, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";
import { ItemCard } from "@/lib/items";

import { TrackerFormSheet } from "./TrackerFormSheet";

/** The user's general trackers and the way to create one (PRD K1, T5). */
export function TrackersScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const [creating, setCreating] = useState(false);
  const trackers = useQuery({
    queryKey: ["trackers"],
    queryFn: () => api.items.filter({ kinds: ["tracker"] }, { limit: 200 }),
  });
  const openTracker = (id: string) => {
    const href = `/trackers/${encodeURIComponent(id)}` as Href;
    router.push(href);
  };
  const startCreating = () => {
    setCreating(true);
  };

  return (
    <Screen title={t("nav.trackers")}>
      {trackers.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : trackers.isError ? (
        <EmptyState
          title={t("trackers.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void trackers.refetch();
            },
          }}
        />
      ) : trackers.data.items.length === 0 ? (
        <EmptyState
          title={t("trackers.emptyTitle")}
          description={t("trackers.emptyHint")}
          action={{ title: t("trackers.new"), onPress: startCreating }}
        />
      ) : (
        <View className="gap-3">
          <View className="self-start">
            <Button title={t("trackers.new")} size="sm" onPress={startCreating} />
          </View>
          {trackers.data.items.map((item) => (
            <ItemCard
              key={item.id}
              item={item}
              onPress={() => {
                openTracker(item.id);
              }}
            />
          ))}
        </View>
      )}
      <TrackerFormSheet
        visible={creating}
        onClose={() => {
          setCreating(false);
        }}
        onCreated={(tracker) => {
          setCreating(false);
          openTracker(tracker.item.id);
        }}
      />
    </Screen>
  );
}
