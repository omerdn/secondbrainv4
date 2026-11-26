import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Item } from "@secondbrain/api-types";
import { Button, EmptyState, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { useSession } from "@/features/auth";
import { Screen } from "@/features/shell";

import { NameSheet } from "./NameSheet";
import { PageEditor } from "./PageEditor";
import { PageLinks } from "./PageLinks";

/** A page of a notebook: its collaborative editor (web, T2.5.7), rename and delete. The mobile
 * editor comes with T4.2.2 (ADR-0024). */
export function PageScreen({ id }: { id: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const router = useRouter();
  const { state: session } = useSession();
  const [renaming, setRenaming] = useState(false);
  const [confirming, setConfirming] = useState(false);
  const page = useQuery({ queryKey: ["page", id], queryFn: () => api.notebooks.page(id) });
  const refresh = async (notebook: string) => {
    await client.invalidateQueries({ queryKey: ["page", id] });
    await client.invalidateQueries({ queryKey: ["notebook", notebook] });
  };
  const rename = useIdempotentMutation<Item, string>(
    (title, idempotencyKey) => api.items.update(id, { title }, { idempotencyKey }),
    {
      onSuccess: async () => {
        if (page.data !== undefined) await refresh(page.data.notebook_id);
        setRenaming(false);
      },
    },
  );
  const remove = useIdempotentMutation<void, string>(
    (pageId, idempotencyKey) => api.items.trash(pageId, { idempotencyKey }),
    {
      onSuccess: async () => {
        if (page.data !== undefined) await refresh(page.data.notebook_id);
        const href = "/notebooks" as Href;
        router.replace(href);
      },
    },
  );

  if (page.isPending) {
    return <Screen title={t("common.loading")}>{null}</Screen>;
  }
  if (page.isError) {
    const notFound = page.error instanceof ApiError && page.error.status === 404;
    return (
      <Screen title={t("nav.notebooks")}>
        {notFound ? (
          <EmptyState
            title={t("items.detail.notFound")}
            description={t("items.detail.notFoundHint")}
          />
        ) : (
          <EmptyState
            title={t("notebooks.loadFailed")}
            action={{
              title: t("items.detail.retry"),
              onPress: () => {
                void page.refetch();
              },
            }}
          />
        )}
      </Screen>
    );
  }

  const { item } = page.data;
  return (
    <Screen title={item.title}>
      <View className="gap-4">
        <View className="flex-row flex-wrap gap-2">
          <Button
            title={t("notebooks.rename")}
            size="sm"
            variant="ghost"
            onPress={() => {
              rename.reset();
              setRenaming(true);
            }}
          />
          {confirming ? (
            <>
              <Button
                title={t("notebooks.confirmDelete")}
                size="sm"
                variant="danger"
                loading={remove.isPending}
                onPress={() => {
                  remove.mutate(item.id);
                }}
              />
              <Button
                title={t("notebooks.cancel")}
                size="sm"
                variant="ghost"
                onPress={() => {
                  setConfirming(false);
                }}
              />
            </>
          ) : (
            <Button
              title={t("notebooks.delete")}
              size="sm"
              variant="dangerOutline"
              onPress={() => {
                setConfirming(true);
              }}
            />
          )}
        </View>
        {remove.isError ? (
          <Text tone="danger" accessibilityRole="alert">
            {t("notebooks.saveFailed")}
          </Text>
        ) : null}
        <PageLinks pageId={item.id} />
        {session.status === "signedIn" ? (
          <PageEditor
            key={item.id}
            pageId={item.id}
            user={{ id: session.user.id, name: session.user.display_name }}
          />
        ) : null}
      </View>
      <NameSheet
        visible={renaming}
        title={t("notebooks.rename")}
        submitLabel={t("notebooks.save")}
        initial={item.title}
        saving={rename.isPending}
        failed={rename.isError}
        onClose={() => {
          setRenaming(false);
        }}
        onSubmit={(title) => {
          rename.mutate(title);
        }}
      />
    </Screen>
  );
}
