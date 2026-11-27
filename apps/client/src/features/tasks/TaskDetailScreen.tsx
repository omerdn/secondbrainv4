import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Task } from "@secondbrain/api-types";
import { Button, Card, EmptyState, TagChip, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";
import { TagPicker } from "@/features/tags";

import {
  datesPatch,
  type DraftErrors,
  draftErrors,
  draftOf,
  itemPatch,
  type TaskDraft,
} from "./model";
import { SubtaskList } from "./SubtaskList";
import { TaskFields } from "./TaskFields";

function TaskEditor({ task }: { task: Task }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const id = task.item.id;
  const [draft, setDraft] = useState<TaskDraft>(() => draftOf(task));
  const [errors, setErrors] = useState<DraftErrors>({});
  const [picking, setPicking] = useState(false);
  const readOnly = task.item.deleted_at !== null;
  const tags = useQuery({
    queryKey: ["item", id, "tags"],
    queryFn: () => api.items.tags(id),
    initialData: task.tags,
  });
  const save = useIdempotentMutation<void, TaskDraft>(
    async (next, idempotencyKey) => {
      const fields = itemPatch(task, next);
      const dates = datesPatch(task, next);
      if (fields !== null) {
        await api.items.update(id, fields, { idempotencyKey: `${idempotencyKey}-item` });
      }
      if (dates !== null) {
        await api.tasks.update(id, dates, { idempotencyKey: `${idempotencyKey}-dates` });
      }
    },
    {
      onSuccess: async () => {
        await Promise.all([
          client.invalidateQueries({ queryKey: ["task", id] }),
          client.invalidateQueries({ queryKey: ["item", id] }),
          client.invalidateQueries({ queryKey: ["tasks"] }),
        ]);
      },
    },
  );
  const dirty = itemPatch(task, draft) !== null || datesPatch(task, draft) !== null;

  return (
    <View className="gap-4">
      <Card className="gap-3">
        <TaskFields
          draft={draft}
          errors={errors}
          onChange={(changes) => {
            setDraft((current) => ({ ...current, ...changes }));
          }}
        />
        {save.isError ? (
          <Text tone="danger" accessibilityRole="alert">
            {t("tasks.saveFailed")}
          </Text>
        ) : null}
        {readOnly ? null : (
          <Button
            title={t("tasks.save")}
            loading={save.isPending}
            disabled={save.isPending || !dirty}
            onPress={() => {
              const found = draftErrors(draft);
              setErrors(found);
              if (Object.keys(found).length === 0) {
                save.mutate(draft);
              }
            }}
          />
        )}
      </Card>
      <Card className="gap-3">
        <Text variant="label">{t("tasks.subtasks")}</Text>
        <SubtaskList taskId={id} subtasks={task.subtasks} readOnly={readOnly} />
      </Card>
      <Card className="gap-3">
        <Text variant="label">{t("nav.tags")}</Text>
        {tags.data.length === 0 ? (
          <Text tone="muted">{t("tasks.noTags")}</Text>
        ) : (
          <View className="flex-row flex-wrap gap-2">
            {tags.data.map((tag) => (
              <TagChip key={tag.id} label={tag.name} />
            ))}
          </View>
        )}
        {readOnly ? null : (
          <View className="self-start">
            <Button
              title={t("tags.edit")}
              variant="secondary"
              size="sm"
              onPress={() => {
                setPicking(true);
              }}
            />
          </View>
        )}
      </Card>
      <TagPicker
        itemId={id}
        visible={picking}
        onClose={() => {
          setPicking(false);
        }}
      />
    </View>
  );
}

/** A task and everything editable on it (PRD G1, G2). */
export function TaskDetailScreen({ id }: { id: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const task = useQuery({ queryKey: ["task", id], queryFn: () => api.tasks.get(id) });
  if (task.isPending) {
    return <Screen title={t("common.loading")}>{null}</Screen>;
  }
  if (task.isError) {
    const notFound = task.error instanceof ApiError && task.error.status === 404;
    return (
      <Screen title={t("nav.tasks")}>
        {notFound ? (
          <EmptyState
            title={t("items.detail.notFound")}
            description={t("items.detail.notFoundHint")}
          />
        ) : (
          <EmptyState
            title={t("tasks.loadFailed")}
            action={{
              title: t("items.detail.retry"),
              onPress: () => {
                void task.refetch();
              },
            }}
          />
        )}
      </Screen>
    );
  }
  return (
    <Screen title={task.data.item.title}>
      <TaskEditor task={task.data} />
      <View className="self-start">
        <Button
          title={t("trackers.itemDetails")}
          variant="ghost"
          size="sm"
          onPress={() => {
            const href = `/items/${encodeURIComponent(task.data.item.id)}` as Href;
            router.push(href);
          }}
        />
      </View>
    </Screen>
  );
}
