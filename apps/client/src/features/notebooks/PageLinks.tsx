import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { AccessPreviewRequest, Item, ItemLink } from "@secondbrain/api-types";
import { Button, Card, Input, Sheet, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { ShareConfirmDialog } from "@/lib/share";

import { PAGE_TARGET_KINDS, pageLinkCandidates } from "./model";

/**
 * The items a page is linked to (PRD B1). Linking widens who sees the page (V2), so it is only
 * offered with LINK on the page (V4) and the people who would gain access are shown before it
 * happens (V7).
 */
export function PageLinks({ pageId }: { pageId: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const [picking, setPicking] = useState(false);
  const [query, setQuery] = useState("");
  const [target, setTarget] = useState<Item | null>(null);
  const links = useQuery({
    queryKey: ["links", pageId],
    queryFn: () => api.items.links(pageId),
  });
  const access = useQuery({
    queryKey: ["access", "explanation", pageId],
    queryFn: () => api.access.explanation(pageId),
  });
  const canLink = access.data?.permissions.includes("link") === true;
  const candidates = useQuery({
    queryKey: ["items", "filter", "page-link-targets"],
    queryFn: () => api.items.filter({ kinds: [...PAGE_TARGET_KINDS] }, { limit: 200 }),
    enabled: picking,
  });
  const refresh = () => client.invalidateQueries({ queryKey: ["links", pageId] });
  const link = useIdempotentMutation<ItemLink, string>(
    (targetId, idempotencyKey) => api.items.link(pageId, targetId, { idempotencyKey }),
    {
      onSuccess: async () => {
        await refresh();
        setTarget(null);
        setPicking(false);
        setQuery("");
      },
    },
  );
  const unlink = useIdempotentMutation<void, string>(
    (targetId, idempotencyKey) => api.items.unlink(pageId, targetId, { idempotencyKey }),
    { onSuccess: refresh },
  );
  const outgoing = (links.data ?? []).filter((l) => l.direction === "outgoing");
  const linked = new Set(outgoing.map((l) => l.item.id));
  const request: AccessPreviewRequest | null =
    target === null ? null : { action: "link", page_id: pageId, item_id: target.id };

  return (
    <Card className="gap-2">
      <Text className="font-semibold">{t("notebooks.links.title")}</Text>
      {outgoing.length === 0 ? (
        <Text tone="muted">{t("notebooks.links.none")}</Text>
      ) : (
        outgoing.map((l) => (
          <View key={l.item.id} className="flex-row items-center gap-2">
            <Text className="flex-1">{l.item.title}</Text>
            {canLink ? (
              <Button
                title={t("notebooks.links.remove")}
                size="sm"
                variant="ghost"
                accessibilityLabel={t("notebooks.links.removeOf", { title: l.item.title })}
                onPress={() => {
                  unlink.mutate(l.item.id);
                }}
              />
            ) : null}
          </View>
        ))
      )}
      {canLink ? (
        <View className="self-start">
          <Button
            title={t("notebooks.links.add")}
            size="sm"
            variant="secondary"
            onPress={() => {
              setPicking(true);
            }}
          />
        </View>
      ) : null}
      {link.isError || unlink.isError ? (
        <Text tone="danger" accessibilityRole="alert">
          {t("notebooks.saveFailed")}
        </Text>
      ) : null}
      <Sheet
        visible={picking && target === null}
        onClose={() => {
          setPicking(false);
        }}
        title={t("notebooks.links.pick")}
        closeLabel={t("tags.close")}
      >
        <View className="gap-2">
          <Input label={t("notebooks.links.search")} value={query} onChangeText={setQuery} />
          {candidates.isPending ? (
            <Text tone="muted">{t("common.loading")}</Text>
          ) : (
            pageLinkCandidates(candidates.data?.items ?? [], linked, query).map((item) => (
              <Pressable
                key={item.id}
                accessibilityRole="button"
                accessibilityLabel={t("notebooks.links.linkTo", { title: item.title })}
                onPress={() => {
                  link.reset();
                  setTarget(item);
                }}
                className="rounded-md px-2 py-1.5 active:opacity-70"
              >
                <Text>{item.title}</Text>
              </Pressable>
            ))
          )}
        </View>
      </Sheet>
      <ShareConfirmDialog
        request={request}
        confirmLabel={t("notebooks.links.confirm")}
        busy={link.isPending}
        onConfirm={() => {
          if (target !== null) link.mutate(target.id);
        }}
        onCancel={() => {
          setTarget(null);
        }}
      />
    </Card>
  );
}
