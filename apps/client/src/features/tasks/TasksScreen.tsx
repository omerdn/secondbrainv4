import { Button, TagChip } from "@secondbrain/ui";
import { useQueryClient } from "@tanstack/react-query";
import { type Href, useLocalSearchParams, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";

import { KanbanBoard } from "./KanbanBoard";
import { TaskFormSheet } from "./TaskFormSheet";
import { TaskList } from "./TaskList";

/** The user's tasks and the way to create one. */
export function TasksScreen() {
  const { t } = useTranslation();
  const router = useRouter();
  const client = useQueryClient();
  const [creating, setCreating] = useState(false);
  const { view } = useLocalSearchParams<{ view?: string }>();
  const showBoard = view === "board";
  const openTask = (id: string) => {
    const href = `/tasks/${encodeURIComponent(id)}` as Href;
    router.push(href);
  };
  const startCreating = () => {
    setCreating(true);
  };
  return (
    <Screen title={t("nav.tasks")}>
      <View className="flex-row items-center gap-2">
        <TagChip
          label={t("board.listView")}
          selected={!showBoard}
          onPress={() => {
            router.setParams({ view: "list" });
          }}
        />
        <TagChip
          label={t("board.boardView")}
          selected={showBoard}
          onPress={() => {
            router.setParams({ view: "board" });
          }}
        />
        {showBoard ? <Button title={t("tasks.new")} size="sm" onPress={startCreating} /> : null}
      </View>
      {showBoard ? <KanbanBoard /> : <TaskList onOpen={openTask} onCreate={startCreating} />}
      <TaskFormSheet
        visible={creating}
        onClose={() => {
          setCreating(false);
        }}
        onCreated={(task) => {
          setCreating(false);
          void client.invalidateQueries({ queryKey: ["board"] });
          openTask(task.item.id);
        }}
      />
    </Screen>
  );
}
