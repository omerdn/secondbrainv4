import { useApi } from "@secondbrain/api-client/react";
import { Button, EmptyState, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";
import { ItemCard } from "@/lib/items";

import { GoalFormSheet } from "./GoalFormSheet";

/** The user's goals and the way to create one (PRD D1). */
export function GoalsScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const [creating, setCreating] = useState(false);
  const goals = useQuery({
    queryKey: ["goals"],
    queryFn: () => api.items.filter({ kinds: ["goal"] }, { limit: 200 }),
  });
  const openGoal = (id: string) => {
    const href = `/goals/${encodeURIComponent(id)}` as Href;
    router.push(href);
  };
  const startCreating = () => {
    setCreating(true);
  };

  return (
    <Screen title={t("nav.goals")}>
      {goals.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : goals.isError ? (
        <EmptyState
          title={t("goals.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void goals.refetch();
            },
          }}
        />
      ) : goals.data.items.length === 0 ? (
        <EmptyState
          title={t("goals.emptyTitle")}
          description={t("goals.emptyHint")}
          action={{ title: t("goals.new"), onPress: startCreating }}
        />
      ) : (
        <View className="gap-3">
          <View className="self-start">
            <Button title={t("goals.new")} size="sm" onPress={startCreating} />
          </View>
          {goals.data.items.map((item) => (
            <ItemCard
              key={item.id}
              item={item}
              onPress={() => {
                openGoal(item.id);
              }}
            />
          ))}
        </View>
      )}
      <GoalFormSheet
        visible={creating}
        onClose={() => {
          setCreating(false);
        }}
        onCreated={(goal) => {
          setCreating(false);
          openGoal(goal.item.id);
        }}
      />
    </Screen>
  );
}
