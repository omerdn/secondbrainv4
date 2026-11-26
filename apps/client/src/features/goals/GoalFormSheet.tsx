import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { CreateGoalRequest, Goal } from "@secondbrain/api-types";
import { Button, Input, Sheet, Text } from "@secondbrain/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import {
  emptyGoalForm,
  type GoalErrors,
  type GoalField,
  type GoalForm,
  goalErrors,
  goalRequest,
} from "./model";

/** Creates a goal; both dates are required (PRD D1). What was typed stays if saving fails. */
export function GoalFormSheet({
  visible,
  onClose,
  onCreated,
}: {
  visible: boolean;
  onClose: () => void;
  onCreated: (goal: Goal) => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const [form, setForm] = useState<GoalForm>(emptyGoalForm);
  const [errors, setErrors] = useState<GoalErrors>({});
  const create = useIdempotentMutation<Goal, CreateGoalRequest>(
    (body, idempotencyKey) => api.goals.create(body, { idempotencyKey }),
    {
      onSuccess: async (goal) => {
        await client.invalidateQueries({ queryKey: ["goals"] });
        setForm(emptyGoalForm);
        setErrors({});
        onCreated(goal);
      },
    },
  );
  const update = (changes: Partial<GoalForm>) => {
    setForm((current) => ({ ...current, ...changes }));
  };
  const errorOf = (field: GoalField) => {
    const error = errors[field];
    return error === undefined ? undefined : t(`goals.errors.${error}`);
  };
  const submit = () => {
    const request = goalRequest(form);
    if (request === null) {
      setErrors(goalErrors(form));
      return;
    }
    setErrors({});
    create.mutate(request);
  };

  return (
    <Sheet
      visible={visible}
      onClose={onClose}
      title={t("goals.new")}
      closeLabel={t("tags.close")}
      footer={
        <Button
          title={t("goals.create")}
          loading={create.isPending}
          disabled={create.isPending}
          onPress={submit}
        />
      }
    >
      <View className="gap-3">
        <Input
          label={t("goals.fields.title")}
          value={form.title}
          error={errorOf("title")}
          onChangeText={(title) => {
            update({ title });
          }}
        />
        <Input
          label={t("goals.fields.description")}
          value={form.description}
          multiline
          onChangeText={(description) => {
            update({ description });
          }}
        />
        <Input
          label={t("goals.fields.startDate")}
          placeholder="2026-12-01"
          value={form.startDate}
          error={errorOf("startDate")}
          onChangeText={(startDate) => {
            update({ startDate });
          }}
        />
        <Input
          label={t("goals.fields.endDate")}
          placeholder="2026-12-31"
          value={form.endDate}
          error={errorOf("endDate")}
          onChangeText={(endDate) => {
            update({ endDate });
          }}
        />
        {create.isError ? (
          <Text tone="danger" accessibilityRole="alert">
            {t("goals.saveFailed")}
          </Text>
        ) : null}
      </View>
    </Sheet>
  );
}
