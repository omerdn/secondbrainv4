import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { CreateTaskRequest, Task } from "@secondbrain/api-types";
import { Button, Input, Sheet, Text } from "@secondbrain/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { createRequest, type DraftErrors, draftErrors, emptyDraft, type TaskDraft } from "./model";
import { TaskFields } from "./TaskFields";

/** Creates a task. What was typed stays in the form if saving fails. */
export function TaskFormSheet({
  visible,
  onClose,
  onCreated,
}: {
  visible: boolean;
  onClose: () => void;
  onCreated: (task: Task) => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const [draft, setDraft] = useState<TaskDraft>(emptyDraft);
  const [subtasks, setSubtasks] = useState<string[]>([]);
  const [errors, setErrors] = useState<DraftErrors>({});
  const create = useIdempotentMutation<Task, CreateTaskRequest>(
    (body, idempotencyKey) => api.tasks.create(body, { idempotencyKey }),
    {
      onSuccess: async (task) => {
        await client.invalidateQueries({ queryKey: ["tasks"] });
        setDraft(emptyDraft);
        setSubtasks([]);
        setErrors({});
        onCreated(task);
      },
    },
  );
  const submit = () => {
    const found = draftErrors(draft);
    setErrors(found);
    if (Object.keys(found).length === 0) {
      create.mutate(createRequest(draft, subtasks));
    }
  };
  return (
    <Sheet
      visible={visible}
      onClose={onClose}
      title={t("tasks.new")}
      closeLabel={t("tags.close")}
      footer={
        <Button
          title={t("tasks.create")}
          loading={create.isPending}
          disabled={create.isPending}
          onPress={submit}
        />
      }
    >
      <View className="gap-3">
        <TaskFields
          draft={draft}
          errors={errors}
          onChange={(changes) => {
            setDraft((current) => ({ ...current, ...changes }));
          }}
        />
        <Text variant="label">{t("tasks.subtasks")}</Text>
        <Text variant="caption" tone="muted">
          {t("tasks.subtasksHint")}
        </Text>
        {subtasks.map((title, index) => (
          <View key={index} className="flex-row items-end gap-2">
            <Input
              className="flex-1"
              label={t("tasks.subtaskNumber", { number: index + 1 })}
              value={title}
              maxLength={500}
              onChangeText={(value) => {
                setSubtasks((rows) => rows.map((row, at) => (at === index ? value : row)));
              }}
            />
            <Button
              title={t("tasks.remove")}
              accessibilityLabel={t("tasks.removeNumber", { number: index + 1 })}
              variant="ghost"
              size="sm"
              onPress={() => {
                setSubtasks((rows) => rows.filter((_, at) => at !== index));
              }}
            />
          </View>
        ))}
        <View className="self-start">
          <Button
            title={t("tasks.addSubtask")}
            variant="secondary"
            size="sm"
            onPress={() => {
              setSubtasks((rows) => [...rows, ""]);
            }}
          />
        </View>
        {create.isError ? (
          <Text tone="danger" accessibilityRole="alert">
            {t("tasks.saveFailed")}
          </Text>
        ) : null}
      </View>
    </Sheet>
  );
}
