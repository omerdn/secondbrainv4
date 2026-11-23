import { useApi } from "@secondbrain/api-client/react";
import { Button, EmptyState, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";
import { ItemCard } from "@/lib/items";

import { TaskFormSheet } from "./TaskFormSheet";

/** The user's tasks and the way to create one. */
export function TasksScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const [creating, setCreating] = useState(false);
  const tasks = useQuery({
    queryKey: ["tasks"],
    queryFn: () => api.items.filter({ kinds: ["task"] }, { limit: 200 }),
  });
  const openTask = (id: string) => {
    const href = `/tasks/${encodeURIComponent(id)}` as Href;
    router.push(href);
  };
  const startCreating = () => {
    setCreating(true);
  };
  return (
    <Screen title={t("nav.tasks")}>
      {tasks.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : tasks.isError ? (
        <EmptyState
          title={t("tasks.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void tasks.refetch();
            },
          }}
        />
      ) : tasks.data.items.length === 0 ? (
        <EmptyState
          title={t("tasks.emptyTitle")}
          description={t("tasks.emptyHint")}
          action={{ title: t("tasks.new"), onPress: startCreating }}
        />
      ) : (
        <View className="gap-3">
          <View className="self-start">
            <Button title={t("tasks.new")} size="sm" onPress={startCreating} />
          </View>
          {tasks.data.items.map((item) => (
            <ItemCard
              key={item.id}
              item={item}
              onPress={() => {
                openTask(item.id);
              }}
            />
          ))}
        </View>
      )}
      <TaskFormSheet
        visible={creating}
        onClose={() => {
          setCreating(false);
        }}
        onCreated={(task) => {
          setCreating(false);
          openTask(task.item.id);
        }}
      />
    </Screen>
  );
}
