import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { CreateTrackerRequest, Tracker } from "@secondbrain/api-types";
import { Button, Input, Sheet, TagChip, Text } from "@secondbrain/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import {
  AGGREGATIONS,
  DIRECTIONS,
  emptyForm,
  type FormErrors,
  type FormField,
  PERIODS,
  type TrackerForm,
  validateForm,
} from "./model";

function Choice<T extends string>({
  label,
  options,
  value,
  optionLabel,
  onChange,
}: {
  label: string;
  options: readonly T[];
  value: T;
  optionLabel: (option: T) => string;
  onChange: (option: T) => void;
}) {
  return (
    <View className="gap-1.5">
      <Text variant="label">{label}</Text>
      <View className="flex-row flex-wrap gap-2">
        {options.map((option) => (
          <TagChip
            key={option}
            label={optionLabel(option)}
            selected={option === value}
            onPress={() => {
              onChange(option);
            }}
          />
        ))}
      </View>
    </View>
  );
}

/** Creates a tracker (PRD K1, T1). What was typed stays in the form if saving fails. */
export function TrackerFormSheet({
  visible,
  onClose,
  onCreated,
}: {
  visible: boolean;
  onClose: () => void;
  onCreated: (tracker: Tracker) => void;
}) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const [form, setForm] = useState<TrackerForm>(emptyForm);
  const [errors, setErrors] = useState<FormErrors>({});
  const create = useIdempotentMutation<Tracker, CreateTrackerRequest>(
    (body, idempotencyKey) => api.trackers.create(body, { idempotencyKey }),
    {
      onSuccess: async (tracker) => {
        await client.invalidateQueries({ queryKey: ["trackers"] });
        setForm(emptyForm);
        setErrors({});
        onCreated(tracker);
      },
    },
  );
  const update = (changes: Partial<TrackerForm>) => {
    setForm((current) => ({ ...current, ...changes }));
  };
  const errorOf = (field: FormField) => {
    const error = errors[field];
    return error === undefined ? undefined : t(`trackers.errors.${error}`);
  };
  const submit = () => {
    const result = validateForm(form);
    if (result.errors !== undefined) {
      setErrors(result.errors);
      return;
    }
    setErrors({});
    create.mutate(result.request);
  };

  return (
    <Sheet
      visible={visible}
      onClose={onClose}
      title={t("trackers.new")}
      closeLabel={t("tags.close")}
      footer={
        <Button
          title={t("trackers.create")}
          loading={create.isPending}
          disabled={create.isPending}
          onPress={submit}
        />
      }
    >
      <View className="gap-3">
        <Input
          label={t("trackers.fields.title")}
          value={form.title}
          error={errorOf("title")}
          onChangeText={(title) => {
            update({ title });
          }}
        />
        <Input
          label={t("trackers.fields.unit")}
          hint={t("trackers.fields.unitHint")}
          value={form.unit}
          maxLength={30}
          error={errorOf("unit")}
          onChangeText={(unit) => {
            update({ unit });
          }}
        />
        <Input
          label={t("trackers.fields.target")}
          value={form.target}
          keyboardType="decimal-pad"
          error={errorOf("target")}
          onChangeText={(target) => {
            update({ target });
          }}
        />
        <Choice
          label={t("trackers.fields.direction")}
          options={DIRECTIONS}
          value={form.direction}
          optionLabel={(option) => t(`trackers.direction.${option}`)}
          onChange={(direction) => {
            update({ direction });
          }}
        />
        <Choice
          label={t("trackers.fields.period")}
          options={PERIODS}
          value={form.period}
          optionLabel={(option) => t(`trackers.period.${option}`)}
          onChange={(period) => {
            update({ period });
          }}
        />
        {form.period === "range" ? (
          <View className="gap-3">
            <Input
              label={t("trackers.fields.rangeStart")}
              placeholder="2026-01-31"
              value={form.rangeStart}
              error={errorOf("rangeStart")}
              onChangeText={(rangeStart) => {
                update({ rangeStart });
              }}
            />
            <Input
              label={t("trackers.fields.rangeEnd")}
              placeholder="2026-12-31"
              value={form.rangeEnd}
              error={errorOf("rangeEnd")}
              onChangeText={(rangeEnd) => {
                update({ rangeEnd });
              }}
            />
          </View>
        ) : null}
        <Choice
          label={t("trackers.fields.aggregation")}
          options={AGGREGATIONS}
          value={form.aggregation}
          optionLabel={(option) => t(`trackers.aggregation.${option}`)}
          onChange={(aggregation) => {
            update({ aggregation });
          }}
        />
        {create.isError ? (
          <Text tone="danger" accessibilityRole="alert">
            {t("trackers.saveFailed")}
          </Text>
        ) : null}
      </View>
    </Sheet>
  );
}
