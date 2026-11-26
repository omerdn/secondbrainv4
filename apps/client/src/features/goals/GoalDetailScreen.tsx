import { ApiError } from "@secondbrain/api-client";
import { useApi } from "@secondbrain/api-client/react";
import { Button, Card, EmptyState, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";

import { GoalLinks } from "./GoalLinks";
import { formatDate } from "./model";

/** A goal: its dates and linked items (PRD D1-D3). */
export function GoalDetailScreen({ id }: { id: string }) {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const goal = useQuery({
    queryKey: ["goal", id],
    queryFn: () => api.goals.get(id),
  });

  if (goal.isPending) {
    return <Screen title={t("common.loading")}>{null}</Screen>;
  }
  if (goal.isError) {
    const notFound = goal.error instanceof ApiError && goal.error.status === 404;
    return (
      <Screen title={t("nav.goals")}>
        {notFound ? (
          <EmptyState
            title={t("items.detail.notFound")}
            description={t("items.detail.notFoundHint")}
          />
        ) : (
          <EmptyState
            title={t("goals.loadFailed")}
            action={{
              title: t("items.detail.retry"),
              onPress: () => {
                void goal.refetch();
              },
            }}
          />
        )}
      </Screen>
    );
  }

  const { item, start_date, end_date } = goal.data;
  return (
    <Screen title={item.title}>
      <View className="gap-4">
        <Card className="gap-1">
          <Text tone="muted">
            {t("goals.dates", {
              start: formatDate(start_date, i18n.language),
              end: formatDate(end_date, i18n.language),
            })}
          </Text>
          {item.description !== null ? <Text>{item.description}</Text> : null}
        </Card>
        <GoalLinks goal={goal.data} />
        <View className="self-start">
          <Button
            title={t("trackers.itemDetails")}
            variant="ghost"
            size="sm"
            onPress={() => {
              const href = `/items/${encodeURIComponent(id)}` as Href;
              router.push(href);
            }}
          />
        </View>
      </View>
    </Screen>
  );
}
