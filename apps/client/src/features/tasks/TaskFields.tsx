import { Input } from "@secondbrain/ui";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import type { DraftErrors, DraftField, TaskDraft } from "./model";

/** Title, description and dates (PRD G1), shared by the create form and the detail screen. */
export function TaskFields({
  draft,
  errors,
  onChange,
}: {
  draft: TaskDraft;
  errors: DraftErrors;
  onChange: (changes: Partial<TaskDraft>) => void;
}) {
  const { t } = useTranslation();
  const errorOf = (field: DraftField) => {
    const error = errors[field];
    return error === undefined ? undefined : t(`tasks.errors.${error}`);
  };
  return (
    <View className="gap-3">
      <Input
        label={t("tasks.fields.title")}
        value={draft.title}
        maxLength={500}
        error={errorOf("title")}
        onChangeText={(title) => {
          onChange({ title });
        }}
      />
      <Input
        label={t("tasks.fields.description")}
        value={draft.description}
        multiline
        onChangeText={(description) => {
          onChange({ description });
        }}
      />
      <Input
        label={t("tasks.fields.startDate")}
        placeholder="2026-12-01"
        value={draft.startDate}
        error={errorOf("startDate")}
        onChangeText={(startDate) => {
          onChange({ startDate });
        }}
      />
      <Input
        label={t("tasks.fields.dueDate")}
        placeholder="2026-12-31"
        value={draft.dueDate}
        error={errorOf("dueDate")}
        onChangeText={(dueDate) => {
          onChange({ dueDate });
        }}
      />
    </View>
  );
}
