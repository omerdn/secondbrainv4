import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { ItemFilter, SavedView, UpdateSavedViewRequest } from "@secondbrain/api-types";
import { Button, EmptyState, Input, Sheet, TagChip, Text } from "@secondbrain/ui";
import { useInfiniteQuery, useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";
import { ItemCard } from "@/lib/items";

import { FilterBar } from "./FilterBar";
import { cleanFilter, isEmptyFilter, sameFilter } from "./filterModel";

function Results({ filter }: { filter: ItemFilter }) {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const clean = cleanFilter(filter);
  const results = useInfiniteQuery({
    queryKey: ["items", "filter", clean],
    queryFn: ({ pageParam }) =>
      api.items.filter(clean, pageParam === undefined ? {} : { cursor: pageParam }),
    initialPageParam: undefined as string | undefined,
    getNextPageParam: (last) => last.next_cursor ?? undefined,
  });
  if (results.isPending) {
    return <Text tone="muted">{t("common.loading")}</Text>;
  }
  if (results.isError) {
    return (
      <EmptyState
        title={t("views.loadFailed")}
        action={{
          title: t("items.detail.retry"),
          onPress: () => {
            void results.refetch();
          },
        }}
      />
    );
  }
  const items = results.data.pages.flatMap((page) => page.items);
  if (items.length === 0) {
    return <EmptyState title={t("views.noResults")} />;
  }
  return (
    <View className="gap-2">
      {items.map((item) => (
        <ItemCard
          key={item.id}
          item={item}
          onPress={() => {
            const href = `/items/${encodeURIComponent(item.id)}` as Href;
            router.push(href);
          }}
        />
      ))}
      {results.hasNextPage ? (
        <Button
          title={t("views.loadMore")}
          variant="secondary"
          loading={results.isFetchingNextPage}
          onPress={() => {
            void results.fetchNextPage();
          }}
        />
      ) : null}
    </View>
  );
}

function SaveSheet({
  visible,
  filter,
  onClose,
  onSaved,
}: {
  visible: boolean;
  filter: ItemFilter;
  onClose: () => void;
  onSaved: (view: SavedView) => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const [name, setName] = useState("");
  const [pinned, setPinned] = useState(true);
  const save = useIdempotentMutation<SavedView, { name: string; pinned: boolean }>(
    (body, idempotencyKey) =>
      api.views.create({ ...body, filter: cleanFilter(filter) }, { idempotencyKey }),
    {
      onSuccess: async (view) => {
        await client.invalidateQueries({ queryKey: ["views"] });
        setName("");
        onSaved(view);
      },
    },
  );
  const trimmed = name.trim();
  const message =
    save.error === null
      ? null
      : save.error instanceof ApiError && save.error.status === 409
        ? t("views.nameTaken")
        : t("views.failed");
  return (
    <Sheet
      visible={visible}
      onClose={onClose}
      title={t("views.saveTitle")}
      closeLabel={t("tags.close")}
    >
      <View className="gap-4">
        <Input label={t("views.name")} value={name} onChangeText={setName} maxLength={60} />
        <View className="flex-row">
          <TagChip
            label={t("views.pinToSidebar")}
            selected={pinned}
            onPress={() => {
              setPinned(!pinned);
            }}
          />
        </View>
        <Button
          title={t("views.save")}
          disabled={trimmed === ""}
          loading={save.isPending}
          onPress={() => {
            save.mutate({ name: trimmed, pinned });
          }}
        />
        {message !== null ? (
          <Text tone="danger" accessibilityRole="alert">
            {message}
          </Text>
        ) : null}
      </View>
    </Sheet>
  );
}

function ViewActions({
  view,
  filter,
  onDeleted,
}: {
  view: SavedView;
  filter: ItemFilter;
  onDeleted: () => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const update = useIdempotentMutation<SavedView, UpdateSavedViewRequest>(
    (body, idempotencyKey) => api.views.update(view.id, body, { idempotencyKey }),
    {
      onSuccess: async () => {
        await client.invalidateQueries({ queryKey: ["views"] });
      },
    },
  );
  const remove = useIdempotentMutation<void, string>(
    (id, idempotencyKey) => api.views.delete(id, { idempotencyKey }),
    {
      onSuccess: async () => {
        await client.invalidateQueries({ queryKey: ["views"] });
        onDeleted();
      },
    },
  );
  const dirty = !sameFilter(filter, view.filter);
  return (
    <View className="gap-2">
      <View className="flex-row flex-wrap items-center gap-2">
        <Button
          title={view.pinned ? t("views.unpin") : t("views.pin")}
          variant="secondary"
          size="sm"
          loading={update.isPending}
          onPress={() => {
            update.mutate({ pinned: !view.pinned });
          }}
        />
        {dirty ? (
          <Button
            title={t("views.saveChanges")}
            size="sm"
            loading={update.isPending}
            onPress={() => {
              update.mutate({ filter: cleanFilter(filter) });
            }}
          />
        ) : null}
        {confirming ? (
          <>
            <Button
              title={t("views.confirmDelete")}
              variant="danger"
              size="sm"
              loading={remove.isPending}
              onPress={() => {
                remove.mutate(view.id);
              }}
            />
            <Button
              title={t("views.cancel")}
              variant="ghost"
              size="sm"
              onPress={() => {
                setConfirming(false);
              }}
            />
          </>
        ) : (
          <Button
            title={t("views.delete")}
            variant="dangerOutline"
            size="sm"
            onPress={() => {
              setConfirming(true);
            }}
          />
        )}
      </View>
      {update.isError || remove.isError ? (
        <Text tone="danger" accessibilityRole="alert">
          {t("views.failed")}
        </Text>
      ) : null}
    </View>
  );
}

/** Filters over everything the user can access, and their saved views (PRD F1, F2). */
export function ViewsScreen({ viewId }: { viewId?: string | undefined }) {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const views = useQuery({ queryKey: ["views"], queryFn: api.views.list });
  const tags = useQuery({ queryKey: ["tags"], queryFn: api.tags.list });
  const [draft, setDraft] = useState<ItemFilter | null>(null);
  const [saving, setSaving] = useState(false);
  const selected = views.data?.find((view) => view.id === viewId);
  const filter = draft ?? selected?.filter ?? {};
  const open = (id: string | null) => {
    const href = (id === null ? "/views" : `/views?view=${encodeURIComponent(id)}`) as Href;
    router.replace(href);
  };

  // A link to a view that is still loading must not flash the unfiltered list.
  const waiting = viewId !== undefined && viewId !== "" && views.isPending;
  return (
    <Screen title={selected?.name ?? t("nav.views")}>
      {views.data !== undefined && views.data.length > 0 ? (
        <View className="flex-row flex-wrap gap-2">
          {views.data.map((view) => (
            <TagChip
              key={view.id}
              label={view.name}
              selected={view.id === selected?.id}
              onPress={() => {
                open(view.id === selected?.id ? null : view.id);
              }}
            />
          ))}
        </View>
      ) : null}
      <FilterBar filter={filter} tags={tags.data ?? []} onChange={setDraft} />
      {selected !== undefined ? (
        <ViewActions
          view={selected}
          filter={filter}
          onDeleted={() => {
            open(null);
          }}
        />
      ) : (
        <View className="self-start">
          <Button
            title={t("views.saveAsView")}
            variant="secondary"
            size="sm"
            disabled={isEmptyFilter(filter)}
            onPress={() => {
              setSaving(true);
            }}
          />
        </View>
      )}
      {waiting ? <Text tone="muted">{t("common.loading")}</Text> : <Results filter={filter} />}
      <SaveSheet
        visible={saving}
        filter={filter}
        onClose={() => {
          setSaving(false);
        }}
        onSaved={(view) => {
          setSaving(false);
          open(view.id);
        }}
      />
    </Screen>
  );
}
