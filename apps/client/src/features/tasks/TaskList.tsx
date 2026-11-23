import { useApi } from "@secondbrain/api-client/react";
import type { ItemFilter } from "@secondbrain/api-types";
import { Button, EmptyState, Text } from "@secondbrain/ui";
import { useInfiniteQuery, useQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { FilterBar } from "@/features/views";
import { ItemCard } from "@/lib/items";

import { isDefaultTaskFilter, taskFilter } from "./model";

const PAGE_SIZE = 50;

/** The task list with its filter bar (PRD G7, F1). The kind is always "task". */
export function TaskList({
  onOpen,
  onCreate,
}: {
  onOpen: (id: string) => void;
  onCreate: () => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const [selection, setSelection] = useState<ItemFilter>({});
  const filter = taskFilter(selection);
  const tags = useQuery({ queryKey: ["tags"], queryFn: api.tags.list });
  const tasks = useInfiniteQuery({
    queryKey: ["tasks", "list", filter],
    queryFn: ({ pageParam }) =>
      api.items.filter(filter, { limit: PAGE_SIZE, cursor: pageParam ?? undefined }),
    initialPageParam: null as string | null,
    getNextPageParam: (page) => page.next_cursor,
  });
  const items = tasks.data?.pages.flatMap((page) => page.items) ?? [];

  return (
    <View className="gap-3">
      <View className="self-start">
        <Button title={t("tasks.new")} size="sm" onPress={onCreate} />
      </View>
      <FilterBar
        filter={selection}
        tags={tags.data ?? []}
        showKinds={false}
        onChange={setSelection}
      />
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
      ) : items.length === 0 ? (
        isDefaultTaskFilter(selection) ? (
          <EmptyState title={t("tasks.emptyTitle")} description={t("tasks.emptyHint")} />
        ) : (
          <EmptyState
            title={t("tasks.noMatch")}
            action={{
              title: t("tasks.clearFilter"),
              onPress: () => {
                setSelection({});
              },
            }}
          />
        )
      ) : (
        <View className="gap-3">
          {items.map((item) => (
            <ItemCard
              key={item.id}
              item={item}
              onPress={() => {
                onOpen(item.id);
              }}
            />
          ))}
          {tasks.hasNextPage ? (
            <Button
              title={t("views.loadMore")}
              variant="secondary"
              loading={tasks.isFetchingNextPage}
              disabled={tasks.isFetchingNextPage}
              onPress={() => {
                void tasks.fetchNextPage();
              }}
            />
          ) : null}
        </View>
      )}
    </View>
  );
}
