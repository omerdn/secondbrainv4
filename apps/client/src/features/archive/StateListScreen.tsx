import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Item } from "@secondbrain/api-types";
import { Button, EmptyState, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";
import { ItemCard } from "@/lib/items";

import { trashDaysLeft } from "./trashDays";

/** The list and its way back to the active items (PRD N4). */
function StateList({ state }: { state: "archived" | "trash" }) {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const client = useQueryClient();
  const [failed, setFailed] = useState<{ id: string; message: string } | null>(null);
  const [now] = useState(() => new Date());
  const items = useQuery({
    queryKey: ["items", "state", state],
    queryFn: () => api.items.list(state),
  });
  const bringBack = useIdempotentMutation<Item, string>(
    (id, idempotencyKey) =>
      state === "trash"
        ? api.items.restore(id, { idempotencyKey })
        : api.items.unarchive(id, { idempotencyKey }),
    {
      onMutate: () => {
        setFailed(null);
      },
      onSuccess: async (item) => {
        await Promise.all([
          client.invalidateQueries({ queryKey: ["items"] }),
          client.invalidateQueries({ queryKey: ["item", item.id] }),
        ]);
      },
      onError: (error, id) => {
        const denied = error instanceof ApiError && error.status === 403;
        setFailed({ id, message: t(denied ? "archive.notAllowed" : "archive.failed") });
      },
    },
  );

  if (items.isPending) {
    return <Text tone="muted">{t("common.loading")}</Text>;
  }
  if (items.isError) {
    return (
      <EmptyState
        title={t("archive.loadFailed")}
        action={{
          title: t("items.detail.retry"),
          onPress: () => {
            void items.refetch();
          },
        }}
      />
    );
  }
  if (items.data.length === 0) {
    return (
      <EmptyState
        title={t(state === "trash" ? "archive.trashEmpty" : "archive.archiveEmpty")}
        description={t(state === "trash" ? "archive.trashHint" : "archive.archiveHint")}
      />
    );
  }
  return (
    <View className="gap-3">
      {state === "trash" ? <Text tone="muted">{t("archive.trashHint")}</Text> : null}
      {items.data.map((item) => (
        <View key={item.id} className="gap-1">
          <ItemCard
            item={item}
            onPress={() => {
              const href = `/items/${encodeURIComponent(item.id)}` as Href;
              router.push(href);
            }}
          />
          <View className="flex-row items-center justify-between gap-2">
            <Text variant="caption" tone="muted">
              {state === "trash" && item.deleted_at !== null
                ? t("archive.daysLeft", { count: trashDaysLeft(item.deleted_at, now) })
                : ""}
            </Text>
            <Button
              title={t(state === "trash" ? "archive.restore" : "archive.unarchive")}
              accessibilityLabel={`${t(
                state === "trash" ? "archive.restore" : "archive.unarchive",
              )}: ${item.title}`}
              variant="secondary"
              size="sm"
              loading={bringBack.isPending && bringBack.variables === item.id}
              disabled={bringBack.isPending}
              onPress={() => {
                bringBack.mutate(item.id);
              }}
            />
          </View>
          {failed?.id === item.id ? (
            <Text tone="danger" accessibilityRole="alert">
              {failed.message}
            </Text>
          ) : null}
        </View>
      ))}
    </View>
  );
}

export function ArchiveScreen() {
  const { t } = useTranslation();
  return (
    <Screen title={t("nav.archive")}>
      <StateList state="archived" />
    </Screen>
  );
}

export function TrashScreen() {
  const { t } = useTranslation();
  return (
    <Screen title={t("nav.trash")}>
      <StateList state="trash" />
    </Screen>
  );
}
