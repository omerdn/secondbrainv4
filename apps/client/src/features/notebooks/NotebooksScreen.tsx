import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Item, Notebook, NotebookPage, NotebookSummary } from "@secondbrain/api-types";
import { Button, Card, EmptyState, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

import { Screen } from "@/features/shell";

import { NameSheet } from "./NameSheet";

/** What the name sheet is open for. */
type Naming =
  { kind: "notebook" } | { kind: "page"; notebook: Item } | { kind: "rename"; item: Item };

/** The user's notebooks as a tree of their pages (PRD Y1): create, rename, delete, navigate. */
export function NotebooksScreen() {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const router = useRouter();
  const [naming, setNaming] = useState<Naming | null>(null);
  const [expanded, setExpanded] = useState<Set<string>>(new Set());
  const notebooks = useQuery({ queryKey: ["notebooks"], queryFn: () => api.notebooks.list() });

  const openPage = (id: string) => {
    const href = `/pages/${encodeURIComponent(id)}` as Href;
    router.push(href);
  };
  const toggle = (id: string) => {
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  };
  const refresh = async (notebook?: string) => {
    await client.invalidateQueries({ queryKey: ["notebooks"] });
    if (notebook !== undefined) {
      await client.invalidateQueries({ queryKey: ["notebook", notebook] });
    }
  };

  const createNotebook = useIdempotentMutation<Notebook, string>(
    (title, idempotencyKey) => api.notebooks.create({ title }, { idempotencyKey }),
    {
      onSuccess: async (notebook) => {
        await refresh();
        setNaming(null);
        setExpanded((current) => new Set(current).add(notebook.item.id));
      },
    },
  );
  const createPage = useIdempotentMutation<NotebookPage, { notebook: string; title: string }>(
    ({ notebook, title }, idempotencyKey) =>
      api.notebooks.createPage(notebook, { title }, { idempotencyKey }),
    {
      onSuccess: async (page) => {
        await refresh(page.notebook_id);
        setNaming(null);
        openPage(page.item.id);
      },
    },
  );
  const rename = useIdempotentMutation<Item, { id: string; title: string }>(
    ({ id, title }, idempotencyKey) => api.items.update(id, { title }, { idempotencyKey }),
    {
      onSuccess: async (item) => {
        await refresh(item.id);
        setNaming(null);
      },
    },
  );
  const saving = createNotebook.isPending || createPage.isPending || rename.isPending;
  const failed = createNotebook.isError || createPage.isError || rename.isError;

  const submitName = (title: string) => {
    if (naming === null) return;
    if (naming.kind === "notebook") createNotebook.mutate(title);
    else if (naming.kind === "page") createPage.mutate({ notebook: naming.notebook.id, title });
    else rename.mutate({ id: naming.item.id, title });
  };
  const startNaming = (next: Naming) => {
    createNotebook.reset();
    createPage.reset();
    rename.reset();
    setNaming(next);
  };

  return (
    <Screen title={t("nav.notebooks")}>
      {notebooks.isPending ? (
        <Text tone="muted">{t("common.loading")}</Text>
      ) : notebooks.isError ? (
        <EmptyState
          title={t("notebooks.loadFailed")}
          action={{
            title: t("items.detail.retry"),
            onPress: () => {
              void notebooks.refetch();
            },
          }}
        />
      ) : (
        <View className="gap-3">
          <View className="self-start">
            <Button
              title={t("notebooks.newNotebook")}
              size="sm"
              onPress={() => {
                startNaming({ kind: "notebook" });
              }}
            />
          </View>
          {notebooks.data.map((notebook) => (
            <NotebookNode
              key={notebook.item.id}
              notebook={notebook}
              expanded={expanded.has(notebook.item.id)}
              onToggle={() => {
                toggle(notebook.item.id);
              }}
              onAddPage={() => {
                startNaming({ kind: "page", notebook: notebook.item });
              }}
              onRename={() => {
                startNaming({ kind: "rename", item: notebook.item });
              }}
              onDeleted={() => refresh()}
              onOpenPage={openPage}
              onShare={() => {
                // The item details hold members, roles and the V7 preview (T3.2.5).
                const href = `/items/${encodeURIComponent(notebook.item.id)}` as Href;
                router.push(href);
              }}
            />
          ))}
        </View>
      )}
      <NameSheet
        visible={naming !== null}
        title={
          naming?.kind === "notebook"
            ? t("notebooks.newNotebook")
            : naming?.kind === "page"
              ? t("notebooks.newPageIn", { notebook: naming.notebook.title })
              : t("notebooks.rename")
        }
        submitLabel={naming?.kind === "rename" ? t("notebooks.save") : t("notebooks.create")}
        initial={naming?.kind === "rename" ? naming.item.title : ""}
        saving={saving}
        failed={failed}
        onClose={() => {
          setNaming(null);
        }}
        onSubmit={submitName}
      />
    </Screen>
  );
}

/** One notebook with its pages when expanded. */
function NotebookNode({
  notebook,
  expanded,
  onToggle,
  onAddPage,
  onRename,
  onDeleted,
  onOpenPage,
  onShare,
}: {
  notebook: NotebookSummary;
  expanded: boolean;
  onToggle: () => void;
  onAddPage: () => void;
  onRename: () => void;
  onDeleted: () => Promise<void>;
  onOpenPage: (id: string) => void;
  /** Members and invitations, with the access preview (V7). */
  onShare: () => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const [confirming, setConfirming] = useState(false);
  const { item } = notebook;
  const detail = useQuery({
    queryKey: ["notebook", item.id],
    queryFn: () => api.notebooks.get(item.id),
    enabled: expanded,
  });
  const remove = useIdempotentMutation<void, string>(
    (id, idempotencyKey) => api.items.trash(id, { idempotencyKey }),
    { onSuccess: onDeleted },
  );

  return (
    <Card className="gap-2">
      <Pressable
        accessibilityRole="button"
        accessibilityState={{ expanded }}
        accessibilityLabel={t(expanded ? "notebooks.collapse" : "notebooks.expand", {
          notebook: item.title,
        })}
        onPress={onToggle}
        className="flex-row items-center gap-2"
      >
        <Text>{expanded ? "▾" : "▸"}</Text>
        <Text className="flex-1 font-semibold">{item.title}</Text>
        {notebook.shared ? (
          <View className="rounded-full bg-primary/15 px-2 py-0.5">
            <Text className="text-xs font-semibold text-primary">{t("notebooks.shared")}</Text>
          </View>
        ) : null}
        {notebook.is_default ? (
          <Text tone="muted" className="text-sm">
            {t("notebooks.default")}
          </Text>
        ) : null}
      </Pressable>
      {expanded ? (
        <View className="gap-1 pl-6">
          {detail.isPending ? (
            <Text tone="muted">{t("common.loading")}</Text>
          ) : detail.isError ? (
            <Text tone="danger">{t("notebooks.loadFailed")}</Text>
          ) : detail.data.pages.length === 0 ? (
            <Text tone="muted">{t("notebooks.noPages")}</Text>
          ) : (
            detail.data.pages.map((page) => (
              <Pressable
                key={page.item.id}
                accessibilityRole="link"
                accessibilityLabel={page.item.title}
                onPress={() => {
                  onOpenPage(page.item.id);
                }}
                className="rounded-md px-2 py-1.5 active:opacity-70"
              >
                <Text>{page.item.title}</Text>
              </Pressable>
            ))
          )}
        </View>
      ) : null}
      <View className="flex-row flex-wrap gap-2">
        <Button
          title={t("notebooks.newPage")}
          size="sm"
          variant="ghost"
          accessibilityLabel={t("notebooks.newPageIn", { notebook: item.title })}
          onPress={onAddPage}
        />
        <Button
          title={t("notebooks.rename")}
          size="sm"
          variant="ghost"
          accessibilityLabel={t("notebooks.renameOf", { title: item.title })}
          onPress={onRename}
        />
        <Button
          title={t("notebooks.share")}
          size="sm"
          variant="ghost"
          accessibilityLabel={t("notebooks.shareOf", { title: item.title })}
          onPress={onShare}
        />
        {notebook.is_default ? null : confirming ? (
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
            accessibilityLabel={t("notebooks.deleteOf", { title: item.title })}
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
    </Card>
  );
}
