import { ApiError } from "@secondbrain/api-client";
import { useApi, useIdempotentMutation } from "@secondbrain/api-client/react";
import type {
  CreateTrackerEntryRequest,
  Tracker,
  TrackerEntry,
  TrackerSummary,
} from "@secondbrain/api-types";
import { Button, Card, EmptyState, Input, Text } from "@secondbrain/ui";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { type Href, useRouter } from "expo-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { Screen } from "@/features/shell";

import { HistoryChart } from "./HistoryChart";
import {
  addDays,
  dailySeries,
  formatAmount,
  HISTORY_DAYS,
  normalizeDecimal,
  progressShare,
} from "./model";

/** Most pages read for the chart: 1,000 entries over two weeks. */
const HISTORY_PAGES = 5;
const RECENT_ENTRIES = 10;

function SummaryCard({ tracker, summary }: { tracker: Tracker; summary: TrackerSummary }) {
  const { t, i18n } = useTranslation();
  const amount = (value: string) => `${formatAmount(value, i18n.language)} ${tracker.unit}`;
  const limit = tracker.direction === "limit";
  const share = progressShare(summary.value, summary.target_value);
  const period =
    summary.period_start !== null && summary.period_end !== null
      ? `${summary.period_start} – ${summary.period_end}`
      : t("trackers.period.total");
  return (
    <Card className="gap-2">
      <Text variant="caption" tone="muted">
        {period}
      </Text>
      <Text variant="heading">
        {t(limit ? "trackers.usedOf" : "trackers.doneOf", {
          value: amount(summary.value ?? "0"),
          target: amount(summary.target_value),
        })}
      </Text>
      <View
        accessibilityRole="progressbar"
        accessibilityValue={{ min: 0, max: 100, now: Math.round(share * 100) }}
        className="h-2 overflow-hidden rounded-full bg-border"
      >
        <View
          className={summary.over_limit ? "h-full bg-danger" : "h-full bg-primary"}
          style={{ width: `${share * 100}%` }}
        />
      </View>
      {summary.over_limit ? (
        <Text tone="danger">{t("trackers.overLimit", { value: amount(summary.exceeded_by) })}</Text>
      ) : (
        <Text tone="muted">{t("trackers.remaining", { value: amount(summary.remaining) })}</Text>
      )}
    </Card>
  );
}

function EntryForm({ id, unit }: { id: string; unit: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const client = useQueryClient();
  const [value, setValue] = useState("");
  const [note, setNote] = useState("");
  const [invalid, setInvalid] = useState(false);
  const add = useIdempotentMutation<TrackerEntry, CreateTrackerEntryRequest>(
    (body, idempotencyKey) => api.trackers.addEntry(id, body, { idempotencyKey }),
    {
      onSuccess: async () => {
        setValue("");
        setNote("");
        await client.invalidateQueries({ queryKey: ["tracker", id] });
      },
    },
  );
  const submit = () => {
    const parsed = normalizeDecimal(value);
    if (parsed === null) {
      setInvalid(true);
      return;
    }
    setInvalid(false);
    const trimmed = note.trim();
    add.mutate(trimmed === "" ? { value: parsed } : { value: parsed, note: trimmed });
  };
  return (
    <Card className="gap-3">
      <Text variant="label">{t("trackers.addEntry")}</Text>
      <Input
        label={t("trackers.fields.value", { unit })}
        value={value}
        keyboardType="numbers-and-punctuation"
        error={invalid ? t("trackers.errors.invalidNumber") : undefined}
        onChangeText={setValue}
      />
      <Input
        label={t("trackers.fields.note")}
        value={note}
        maxLength={500}
        onChangeText={setNote}
      />
      {add.isError ? (
        <Text tone="danger" accessibilityRole="alert">
          {t("trackers.saveFailed")}
        </Text>
      ) : null}
      <Button
        title={t("trackers.saveEntry")}
        loading={add.isPending}
        disabled={add.isPending}
        onPress={submit}
      />
    </Card>
  );
}

/** A tracker: its current period, new entries and history (PRD K1–K3). */
export function TrackerDetailScreen({ id }: { id: string }) {
  const { t, i18n } = useTranslation();
  const api = useApi();
  const router = useRouter();
  const tracker = useQuery({
    queryKey: ["tracker", id, "definition"],
    queryFn: () => api.trackers.get(id),
  });
  const summary = useQuery({
    queryKey: ["tracker", id, "summary"],
    queryFn: () => api.trackers.summary(id),
    enabled: tracker.isSuccess,
  });
  const today = summary.data?.today;
  const history = useQuery({
    queryKey: ["tracker", id, "history", today],
    enabled: today !== undefined,
    queryFn: async () => {
      const last = today ?? "";
      const entries: TrackerEntry[] = [];
      let cursor: string | undefined;
      for (let page = 0; page < HISTORY_PAGES; page += 1) {
        const result = await api.trackers.entries(id, {
          from: addDays(last, 1 - HISTORY_DAYS),
          to: last,
          limit: 200,
          cursor,
        });
        entries.push(...result.items);
        if (result.next_cursor === null) {
          break;
        }
        cursor = result.next_cursor;
      }
      return entries;
    },
  });

  if (tracker.isPending) {
    return <Screen title={t("common.loading")}>{null}</Screen>;
  }
  if (tracker.isError) {
    const notFound = tracker.error instanceof ApiError && tracker.error.status === 404;
    return (
      <Screen title={t("nav.trackers")}>
        {notFound ? (
          <EmptyState
            title={t("items.detail.notFound")}
            description={t("items.detail.notFoundHint")}
          />
        ) : (
          <EmptyState
            title={t("trackers.loadFailed")}
            action={{
              title: t("items.detail.retry"),
              onPress: () => {
                void tracker.refetch();
              },
            }}
          />
        )}
      </Screen>
    );
  }

  const { unit, aggregation, item } = tracker.data;
  return (
    <Screen title={item.title}>
      <View className="gap-4">
        {summary.isSuccess ? (
          <SummaryCard tracker={tracker.data} summary={summary.data} />
        ) : summary.isError ? (
          <EmptyState
            title={t("trackers.loadFailed")}
            action={{
              title: t("items.detail.retry"),
              onPress: () => {
                void summary.refetch();
              },
            }}
          />
        ) : (
          <Text tone="muted">{t("common.loading")}</Text>
        )}
        {item.deleted_at === null ? <EntryForm id={id} unit={unit} /> : null}
        <Card className="gap-3">
          <Text variant="label">{t("trackers.history")}</Text>
          {history.isSuccess && today !== undefined ? (
            <HistoryChart series={dailySeries(history.data, aggregation, today)} unit={unit} />
          ) : history.isError ? (
            <Button
              title={t("items.detail.retry")}
              variant="ghost"
              size="sm"
              onPress={() => {
                void history.refetch();
              }}
            />
          ) : (
            <Text tone="muted">{t("common.loading")}</Text>
          )}
          {history.data?.slice(0, RECENT_ENTRIES).map((entry) => (
            <View key={entry.id} className="flex-row justify-between gap-3">
              <Text variant="caption" tone="muted">
                {entry.local_date}
              </Text>
              <Text variant="caption" className="flex-1">
                {entry.note ?? ""}
              </Text>
              <Text variant="label">
                {formatAmount(entry.value, i18n.language)} {unit}
              </Text>
            </View>
          ))}
        </Card>
        <View className="self-start">
          <Button
            title={t("trackers.itemDetails")}
            variant="ghost"
            size="sm"
            onPress={() => {
              const href = `/items/${encodeURIComponent(id)}` as Href;
              router.push(href);
            }}
          />
        </View>
      </View>
    </Screen>
  );
}
