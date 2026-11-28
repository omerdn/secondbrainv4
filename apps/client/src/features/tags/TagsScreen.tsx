import { useApi } from "@secondbrain/api-client/react";
import { Button, EmptyState, TagChip, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { type Href, useLocalSearchParams, useRouter } from "expo-router";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";
import { ItemCard } from "@/lib/items";
import { useTagColors } from "@/lib/tags";

function TagItems({ tagId }: { tagId: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const items = useQuery({
    queryKey: ["tags", tagId, "items"],
    queryFn: () => api.tags.items(tagId),
  });
  if (items.isPending) {
    return <Text tone="muted">{t("common.loading")}</Text>;
  }
  if (items.isError) {
    return (
      <View className="flex-row items-center gap-2">
        <Text tone="danger">{t("tags.loadFailed")}</Text>
        <Button
          title={t("items.detail.retry")}
          variant="ghost"
          size="sm"
          onPress={() => {
            void items.refetch();
          }}
        />
      </View>
    );
  }
  if (items.data.length === 0) {
    return <EmptyState title={t("tags.noItems")} />;
  }
  return (
    <View className="gap-2">
      {items.data.map((item) => (
        <ItemCard
          key={item.id}
          item={item}
          onPress={() => {
            const href = `/items/${encodeURIComponent(item.id)}` as Href;
            router.push(href);
          }}
        />
      ))}
    </View>
  );
}

/** The tags the user can see; choosing one lists its items (PRD E4). */
export function TagsScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const { tag: selectedParam } = useLocalSearchParams<{ tag?: string }>();
  const tags = useQuery({ queryKey: ["tags"], queryFn: api.tags.list });
  const colorOf = useTagColors(tags.data);
  const selected = tags.data?.find((tag) => tag.id === selectedParam);

  return (
    <Screen title={t("nav.tags")}>
      {tags.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : tags.isError ? (
        <EmptyState
          title={t("tags.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void tags.refetch();
            },
          }}
        />
      ) : tags.data.length === 0 ? (
        <EmptyState title={t("tags.emptyTitle")} description={t("tags.emptyHint")} />
      ) : (
        <View className="gap-4">
          <View className="flex-row flex-wrap gap-2">
            {tags.data.map((tag) => (
              <TagChip
                key={tag.id}
                label={tag.name}
                teamColor={colorOf(tag)}
                selected={tag.id === selected?.id}
                onPress={() => {
                  router.setParams({ tag: tag.id === selected?.id ? "" : tag.id });
                }}
              />
            ))}
          </View>
          {selected !== undefined ? (
            <TagItems tagId={selected.id} />
          ) : (
            <Text tone="muted">{t("tags.chooseHint")}</Text>
          )}
        </View>
      )}
    </Screen>
  );
}
