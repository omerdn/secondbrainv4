import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { Subtask } from "@secondbrain/api-types";
import { Button, cx, Input, Text } from "@secondbrain/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { Pressable, View } from "react-native";

type Change =
  | { type: "toggle"; subtask: Subtask }
  | { type: "rename"; subtask: Subtask; title: string }
  | { type: "delete"; subtask: Subtask }
  | { type: "add"; title: string };

/** The task's subtasks: add, rename, check and delete (PRD G2, G4). */
export function SubtaskList({
  taskId,
  subtasks,
  readOnly,
}: {
  taskId: string;
  subtasks: readonly Subtask[];
  readOnly: boolean;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const [editing, setEditing] = useState<{ id: string; title: string } | null>(null);
  const [newTitle, setNewTitle] = useState("");
  const [failure, setFailure] = useState<string | null>(null);
  const change = useIdempotentMutation<unknown, Change>(
    (input, idempotencyKey) => {
      const options = { idempotencyKey };
      switch (input.type) {
        case "toggle":
          return input.subtask.done_at === null
            ? api.tasks.completeSubtask(taskId, input.subtask.id, options)
            : api.tasks.reopenSubtask(taskId, input.subtask.id, options);
        case "rename":
          return api.tasks.renameSubtask(taskId, input.subtask.id, { title: input.title }, options);
        case "delete":
          return api.tasks.deleteSubtask(taskId, input.subtask.id, options);
        case "add":
          return api.tasks.addSubtask(taskId, { title: input.title }, options);
      }
    },
    {
      onMutate: () => {
        setFailure(null);
      },
      onSuccess: async (_data, input) => {
        if (input.type === "add") {
          setNewTitle("");
        }
        if (input.type === "rename") {
          setEditing(null);
        }
        await Promise.all([
          client.invalidateQueries({ queryKey: ["task", taskId] }),
          client.invalidateQueries({ queryKey: ["tasks"] }),
        ]);
      },
      onError: (error) => {
        const denied = error instanceof ApiError && error.status === 403;
        setFailure(t(denied ? "tasks.notAllowed" : "tasks.saveFailed"));
      },
    },
  );
  const busy = change.isPending;
  const onlyOne = subtasks.length === 1;

  return (
    <View className="gap-2">
      {subtasks.map((subtask) => {
        const done = subtask.done_at !== null;
        if (editing?.id === subtask.id) {
          const title = editing.title.trim();
          return (
            <View key={subtask.id} className="gap-2">
              <Input
                label={t("tasks.renameSubtask")}
                value={editing.title}
                maxLength={500}
                onChangeText={(value) => {
                  setEditing({ id: subtask.id, title: value });
                }}
              />
              <View className="flex-row gap-2">
                <Button
                  title={t("tasks.save")}
                  size="sm"
                  loading={busy}
                  disabled={busy || title === ""}
                  onPress={() => {
                    change.mutate({ type: "rename", subtask, title });
                  }}
                />
                <Button
                  title={t("tasks.cancel")}
                  variant="ghost"
                  size="sm"
                  onPress={() => {
                    setEditing(null);
                  }}
                />
              </View>
            </View>
          );
        }
        return (
          <View key={subtask.id} className="min-h-10 flex-row items-center gap-2">
            <Pressable
              accessibilityRole="checkbox"
              accessibilityLabel={subtask.title}
              aria-checked={done}
              aria-disabled={readOnly || busy}
              disabled={readOnly || busy}
              onPress={() => {
                change.mutate({ type: "toggle", subtask });
              }}
              className="flex-1 flex-row items-center gap-2 py-1"
            >
              <View
                className={cx(
                  "h-5 w-5 items-center justify-center rounded-sm border",
                  done ? "border-primary bg-primary" : "border-border",
                )}
              >
                {done ? <Text variant="caption">✓</Text> : null}
              </View>
              <Text
                className={cx("flex-1", done && "line-through")}
                tone={done ? "muted" : "default"}
              >
                {subtask.title}
              </Text>
            </Pressable>
            {readOnly ? null : (
              <>
                <Button
                  title={t("tasks.edit")}
                  accessibilityLabel={`${t("tasks.edit")}: ${subtask.title}`}
                  variant="ghost"
                  size="sm"
                  onPress={() => {
                    setEditing({ id: subtask.id, title: subtask.title });
                  }}
                />
                <Button
                  title={t("tasks.remove")}
                  accessibilityLabel={`${t("tasks.remove")}: ${subtask.title}`}
                  variant="ghost"
                  size="sm"
                  disabled={busy || onlyOne}
                  onPress={() => {
                    change.mutate({ type: "delete", subtask });
                  }}
                />
              </>
            )}
          </View>
        );
      })}
      {onlyOne && !readOnly ? (
        <Text variant="caption" tone="muted">
          {t("tasks.lastSubtask")}
        </Text>
      ) : null}
      {readOnly ? null : (
        <View className="gap-2">
          <Input
            label={t("tasks.newSubtask")}
            value={newTitle}
            maxLength={500}
            onChangeText={setNewTitle}
          />
          <View className="self-start">
            <Button
              title={t("tasks.addSubtask")}
              variant="secondary"
              size="sm"
              disabled={busy || newTitle.trim() === ""}
              onPress={() => {
                change.mutate({ type: "add", title: newTitle.trim() });
              }}
            />
          </View>
        </View>
      )}
      {failure !== null ? (
        <Text tone="danger" accessibilityRole="alert">
          {failure}
        </Text>
      ) : null}
    </View>
  );
}
