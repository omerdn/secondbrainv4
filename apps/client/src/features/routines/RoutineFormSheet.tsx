import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type { CreateRoutineRequest, Routine } from "@secondbrain/api-types";
import { Button, Input, Sheet, TagChip, Text } from "@secondbrain/ui";
import { useQueryClient } from "@tanstack/react-query";
import { useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { deviceTimeZone, todayIn } from "@/lib/dates";

import { describePlan, formatDay, formatMonthDay, monthName } from "./describe";
import {
  addReminder,
  buildRrule,
  emptyForm,
  FREQUENCIES,
  type FormErrors,
  type FormField,
  isYearlyDate,
  LAST_DAY,
  nextOccurrences,
  normalizeTime,
  PREVIEW_COUNT,
  type RoutineForm,
  toggle,
  validateForm,
  WEEKDAYS,
} from "./recurrence";

const MONTH_DAYS = Array.from({ length: 31 }, (_, i) => i + 1);
const MONTHS = Array.from({ length: 12 }, (_, i) => i + 1);

function Label({ children }: { children: string }) {
  return <Text variant="label">{children}</Text>;
}

function FieldError({ message }: { message: string | undefined }) {
  return message === undefined ? null : (
    <Text tone="danger" variant="caption">
      {message}
    </Text>
  );
}

/** The next occurrences of the plan as typed so far (T2.2.5 "önizleme"). */
function PlanPreview({ form }: { form: RoutineForm }) {
  const { t, i18n } = useTranslation();
  const interval = Number(form.interval.trim());
  const valid = Number.isInteger(interval) && interval >= 1;
  const rrule = valid ? buildRrule(form, interval) : "";
  const from = todayIn(form.timezone);
  const end = form.endDate.trim() === "" ? undefined : form.endDate.trim();
  const days = useMemo(
    () =>
      rrule === "" ? [] : nextOccurrences(rrule, form.startDate.trim(), from, PREVIEW_COUNT, end),
    [rrule, form.startDate, from, end],
  );
  return (
    <View
      className="gap-1.5 rounded-md border border-border p-3"
      accessibilityLabel={t("routines.preview")}
    >
      <Label>{t("routines.preview")}</Label>
      {rrule === "" ? (
        <Text tone="muted">{t("routines.incomplete")}</Text>
      ) : (
        <View className="gap-1">
          <Text>{describePlan(rrule, t, i18n.language)}</Text>
          {days.length === 0 ? (
            <Text tone="muted">{t("routines.noPreview")}</Text>
          ) : (
            days.map((day) => (
              <Text key={day} tone="muted">
                {formatDay(day, i18n.language)}
              </Text>
            ))
          )}
        </View>
      )}
    </View>
  );
}

/** Creates a routine with its plan, daily target and reminders (PRD R1-R3, R13). */
export function RoutineFormSheet({
  visible,
  onClose,
  onCreated,
}: {
  visible: boolean;
  onClose: () => void;
  onCreated: (routine: Routine) => void;
}) {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const initial = () => {
    const zone = deviceTimeZone();
    return emptyForm(todayIn(zone), zone);
  };
  const [form, setForm] = useState<RoutineForm>(initial);
  const [errors, setErrors] = useState<FormErrors>({});
  const [time, setTime] = useState("");
  const [timeError, setTimeError] = useState(false);
  const [month, setMonth] = useState(1);
  const [day, setDay] = useState("");
  const [dateError, setDateError] = useState(false);
  const create = useIdempotentMutation<Routine, CreateRoutineRequest>(
    (body, idempotencyKey) => api.routines.create(body, { idempotencyKey }),
    {
      onSuccess: async (routine) => {
        await client.invalidateQueries({ queryKey: ["routines"] });
        setForm(initial());
        setErrors({});
        onCreated(routine);
      },
    },
  );
  const update = (changes: Partial<RoutineForm>) => {
    setForm((current) => ({ ...current, ...changes }));
  };
  const errorOf = (field: FormField) => {
    const error = errors[field];
    return error === undefined ? undefined : t(`routines.errors.${error}`);
  };
  const addTime = () => {
    const normalized = normalizeTime(time);
    if (normalized === null) {
      setTimeError(true);
      return;
    }
    setTimeError(false);
    setTime("");
    update({ reminders: addReminder(form.reminders, normalized) });
  };
  const addDate = () => {
    const date = { month, day: Number(day.trim()) };
    if (!/^\d{1,2}$/.test(day.trim()) || !isYearlyDate(date)) {
      setDateError(true);
      return;
    }
    setDateError(false);
    setDay("");
    if (!form.yearlyDates.some((d) => d.month === date.month && d.day === date.day)) {
      update({ yearlyDates: [...form.yearlyDates, date] });
    }
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
      title={t("routines.new")}
      closeLabel={t("tags.close")}
      footer={
        <Button
          title={t("routines.create")}
          loading={create.isPending}
          disabled={create.isPending}
          onPress={submit}
        />
      }
    >
      <View className="gap-3">
        <Input
          label={t("routines.fields.title")}
          value={form.title}
          error={errorOf("title")}
          onChangeText={(title) => {
            update({ title });
          }}
        />
        <View className="gap-1.5">
          <Label>{t("routines.fields.frequency")}</Label>
          <View className="flex-row flex-wrap gap-2">
            {FREQUENCIES.map((frequency) => (
              <TagChip
                key={frequency}
                label={t(`routines.frequency.${frequency}`)}
                selected={form.frequency === frequency}
                onPress={() => {
                  update({ frequency });
                }}
              />
            ))}
          </View>
        </View>
        <Input
          label={t("routines.fields.interval", {
            unit: t(`routines.intervalUnit.${form.frequency}`),
          })}
          value={form.interval}
          keyboardType="number-pad"
          maxLength={2}
          error={errorOf("interval")}
          onChangeText={(interval) => {
            update({ interval });
          }}
        />
        {form.frequency === "weekly" ? (
          <View className="gap-1.5">
            <Label>{t("routines.fields.weekdays")}</Label>
            <View className="flex-row flex-wrap gap-2">
              {WEEKDAYS.map((code) => (
                <TagChip
                  key={code}
                  label={t(`routines.weekday.${code}`)}
                  selected={form.weekdays.includes(code)}
                  onPress={() => {
                    update({ weekdays: toggle(form.weekdays, code) });
                  }}
                />
              ))}
            </View>
            <FieldError message={errorOf("weekdays")} />
          </View>
        ) : null}
        {form.frequency === "monthly" ? (
          <View className="gap-1.5">
            <Label>{t("routines.fields.monthDays")}</Label>
            <View className="flex-row flex-wrap gap-2">
              {[...MONTH_DAYS, LAST_DAY].map((value) => (
                <TagChip
                  key={value}
                  label={value === LAST_DAY ? t("routines.lastDay") : String(value)}
                  selected={form.monthDays.includes(value)}
                  onPress={() => {
                    update({ monthDays: toggle(form.monthDays, value) });
                  }}
                />
              ))}
            </View>
            <FieldError message={errorOf("monthDays")} />
          </View>
        ) : null}
        {form.frequency === "yearly" ? (
          <View className="gap-1.5">
            <Label>{t("routines.fields.month")}</Label>
            <View className="flex-row flex-wrap gap-2">
              {MONTHS.map((value) => (
                <TagChip
                  key={value}
                  label={monthName(value, i18n.language)}
                  selected={month === value}
                  onPress={() => {
                    setMonth(value);
                  }}
                />
              ))}
            </View>
            <View className="flex-row items-end gap-2">
              <View className="flex-1">
                <Input
                  label={t("routines.fields.day")}
                  value={day}
                  keyboardType="number-pad"
                  maxLength={2}
                  error={dateError ? t("routines.errors.invalidDate") : undefined}
                  onChangeText={setDay}
                />
              </View>
              <Button title={t("routines.addDate")} variant="secondary" onPress={addDate} />
            </View>
            <View className="flex-row flex-wrap gap-2">
              {form.yearlyDates.map((date) => {
                const label = formatMonthDay(date.month, date.day, i18n.language);
                return (
                  <TagChip
                    key={`${String(date.month)}-${String(date.day)}`}
                    label={label}
                    removeLabel={t("routines.removeDate", { date: label })}
                    onRemove={() => {
                      update({ yearlyDates: form.yearlyDates.filter((d) => d !== date) });
                    }}
                  />
                );
              })}
            </View>
            <FieldError message={errorOf("yearlyDates")} />
          </View>
        ) : null}
        <Input
          label={t("routines.fields.dailyTarget")}
          value={form.dailyTarget}
          keyboardType="number-pad"
          maxLength={3}
          error={errorOf("dailyTarget")}
          onChangeText={(dailyTarget) => {
            update({ dailyTarget });
          }}
        />
        <Input
          label={t("routines.fields.startDate")}
          value={form.startDate}
          error={errorOf("startDate")}
          onChangeText={(startDate) => {
            update({ startDate });
          }}
        />
        <Input
          label={t("routines.fields.endDate")}
          value={form.endDate}
          error={errorOf("endDate")}
          onChangeText={(endDate) => {
            update({ endDate });
          }}
        />
        <View className="gap-1.5">
          <View className="flex-row items-end gap-2">
            <View className="flex-1">
              <Input
                label={t("routines.fields.reminder")}
                placeholder="08:00"
                value={time}
                maxLength={5}
                error={timeError ? t("routines.errors.invalidTime") : undefined}
                onChangeText={setTime}
                onSubmitEditing={addTime}
              />
            </View>
            <Button title={t("routines.addTime")} variant="secondary" onPress={addTime} />
          </View>
          <View className="flex-row flex-wrap gap-2">
            {form.reminders.map((reminder) => (
              <TagChip
                key={reminder}
                label={reminder}
                removeLabel={t("routines.removeTime", { time: reminder })}
                onRemove={() => {
                  update({ reminders: form.reminders.filter((r) => r !== reminder) });
                }}
              />
            ))}
          </View>
          <FieldError message={errorOf("reminders")} />
        </View>
        <Input
          label={t("routines.fields.timezone")}
          value={form.timezone}
          autoCapitalize="none"
          onChangeText={(timezone) => {
            update({ timezone });
          }}
        />
        <PlanPreview form={form} />
        {create.isError ? (
          <Text tone="danger" accessibilityRole="alert">
            {t("routines.saveFailed")}
          </Text>
        ) : null}
      </View>
    </Sheet>
  );
}
