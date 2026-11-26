import { useApi } from "@secondbrain/api-client/react";
import { Button, EmptyState, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";
import { ItemCard } from "@/lib/items";

import { RoutineFormSheet } from "./RoutineFormSheet";

/** The user's routines and the way to create one (PRD R1-R3). */
export function RoutinesScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const [creating, setCreating] = useState(false);
  const routines = useQuery({
    queryKey: ["routines"],
    queryFn: () => api.items.filter({ kinds: ["routine"] }, { limit: 200 }),
  });
  const openRoutine = (id: string) => {
    const href = `/routines/${encodeURIComponent(id)}` as Href;
    router.push(href);
  };
  const startCreating = () => {
    setCreating(true);
  };

  return (
    <Screen title={t("nav.routines")}>
      {routines.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : routines.isError ? (
        <EmptyState
          title={t("routines.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void routines.refetch();
            },
          }}
        />
      ) : routines.data.items.length === 0 ? (
        <EmptyState
          title={t("routines.emptyTitle")}
          description={t("routines.emptyHint")}
          action={{ title: t("routines.new"), onPress: startCreating }}
        />
      ) : (
        <View className="gap-3">
          <View className="self-start">
            <Button title={t("routines.new")} size="sm" onPress={startCreating} />
          </View>
          {routines.data.items.map((item) => (
            <ItemCard
              key={item.id}
              item={item}
              onPress={() => {
                openRoutine(item.id);
              }}
            />
          ))}
        </View>
      )}
      <RoutineFormSheet
        visible={creating}
        onClose={() => {
          setCreating(false);
        }}
        onCreated={(routine) => {
          setCreating(false);
          openRoutine(routine.item.id);
        }}
      />
    </Screen>
  );
}
