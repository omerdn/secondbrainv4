import { ApiError } from "@secondbrain/api-client";
import { useApi } from "@secondbrain/api-client/react";
import { EmptyState } from "@secondbrain/ui";
import { type UseQueryResult, useQuery } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useTranslation } from "react-i18next";

import { useSession } from "@/features/auth";
import { Screen } from "@/features/shell";
import { ItemDetail, type Loadable, roleLabels } from "@/lib/items";

function loadable<T, V>(query: UseQueryResult<T>, select: (data: T) => V): Loadable<V> {
  if (query.isSuccess) {
    return { status: "ready", value: select(query.data) };
  }
  if (query.isError) {
    return {
      status: "error",
      retry: () => {
        void query.refetch();
      },
    };
  }
  return { status: "loading" };
}

/** Detail of any item. An item the user cannot see looks the same as one that does not exist. */
export function ItemDetailScreen({ id }: { id: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const { state } = useSession();

  const item = useQuery({ queryKey: ["item", id], queryFn: () => api.items.get(id) });
  const enabled = item.isSuccess;
  const access = useQuery({
    queryKey: ["item", id, "access"],
    queryFn: () => api.access.explanation(id),
    enabled,
  });
  const tags = useQuery({
    queryKey: ["item", id, "tags"],
    queryFn: () => api.items.tags(id),
    enabled,
  });
  const links = useQuery({
    queryKey: ["item", id, "links"],
    queryFn: () => api.items.links(id),
    enabled,
  });

  if (item.isPending) {
    return <Screen title={t("common.loading")}>{null}</Screen>;
  }
  if (item.isError) {
    const notFound = item.error instanceof ApiError && item.error.status === 404;
    return (
      <Screen title={t("items.detail.title")}>
        {notFound ? (
          <EmptyState
            title={t("items.detail.notFound")}
            description={t("items.detail.notFoundHint")}
          />
        ) : (
          <EmptyState
            title={t("items.detail.loadFailed")}
            action={{
              title: t("items.detail.retry"),
              onPress: () => {
                void item.refetch();
              },
            }}
          />
        )}
      </Screen>
    );
  }

  const ownedByMe =
    state.status === "signedIn" &&
    item.data.owner.type === "user" &&
    item.data.owner.user_id === state.user.id;
  return (
    <Screen title={item.data.title}>
      <ItemDetail
        item={item.data}
        ownedByMe={ownedByMe}
        roles={loadable(access, (data) => roleLabels(data.reasons))}
        tags={loadable(tags, (data) => data)}
        links={loadable(links, (data) => data)}
        onOpenItem={(target) => {
          const href = `/items/${encodeURIComponent(target)}` as Href;
          router.push(href);
        }}
      />
    </Screen>
  );
}
