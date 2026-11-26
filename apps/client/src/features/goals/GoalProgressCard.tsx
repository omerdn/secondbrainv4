import { useApi } from "@secondbrain/api-client/react";
import type { GoalItemProgress } from "@secondbrain/api-types";
import { Button, Card, Text } from "@secondbrain/ui";
import { useQuery } from "@tanstack/react-query";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { deviceTimeZone, todayIn } from "@/lib/dates";
import { kindLabel } from "@/lib/items";

import { paceTone, percent } from "./model";

const BAR_HEIGHT = 8;

const TONE = { ahead: "success", onTrack: "default", behind: "danger" } as const;

/** A 0-1 share as a bar; `marker` shows where the plan expects it to be. */
function Bar({ share, marker }: { share: number; marker?: number }) {
  return (
    <View className="overflow-hidden rounded-full bg-border" style={{ height: BAR_HEIGHT }}>
      <View
        className="bg-primary"
        style={{ height: BAR_HEIGHT, width: `${String(percent(Math.min(share, 1)))}%` }}
      />
      {marker !== undefined ? (
        <View
          className="absolute bg-foreground"
          style={{
            height: BAR_HEIGHT,
            width: 2,
            left: `${String(percent(Math.min(marker, 1)))}%`,
          }}
        />
      ) : null}
    </View>
  );
}

/** One linked item's part, so the calculation can be followed (PRD D6). */
function BreakdownRow({ part }: { part: GoalItemProgress }) {
  const { t, i18n } = useTranslation();
  const number = (value: number) =>
    new Intl.NumberFormat(i18n.language, { maximumFractionDigits: 2 }).format(value);
  return (
    <View className="gap-1 border-t border-border pt-3">
      <Text variant="caption" tone="muted">
        {t(`items.kinds.${kindLabel(part.item.kind)}`)}
      </Text>
      <Text className="font-semibold">{part.item.title}</Text>
      {part.not_started ? (
        <Text tone="muted" variant="caption">
          {t("goals.notStarted")}
        </Text>
      ) : (
        <View className="gap-1">
          <Bar share={part.actual} marker={part.expected} />
          <Text variant="caption">
            {t("goals.itemActual", {
              actual: percent(part.actual),
              expected: percent(part.expected),
            })}
          </Text>
          <Text variant="caption" tone="muted">
            {t("goals.itemFigures", {
              done: number(part.done),
              total: number(part.total),
              expected: number(part.expected_to_date),
            })}
          </Text>
          <Text variant="caption" tone="muted">
            {t("goals.itemContribution", {
              contribution: percent(part.contribution),
              share: percent(part.share),
            })}
          </Text>
        </View>
      )}
    </View>
  );
}

/** The goal's two indicators together (PRD D4) and each item's contribution (D6). */
export function GoalProgressCard({ goalId }: { goalId: string }) {
  const { t } = useTranslation();
  const api = useApi();
  const today = todayIn(deviceTimeZone());
  const progress = useQuery({
    queryKey: ["goal", goalId, "progress", today],
    queryFn: () => api.goals.progress(goalId, today),
  });

  if (progress.isPending) {
    return (
      <Card>
        <Text tone="muted">{t("common.loading")}</Text>
      </Card>
    );
  }
  if (progress.isError) {
    return (
      <Card className="gap-2">
        <Text tone="danger">{t("goals.progressFailed")}</Text>
        <View className="self-start">
          <Button
            title={t("items.detail.retry")}
            variant="ghost"
            size="sm"
            onPress={() => {
              void progress.refetch();
            }}
          />
        </View>
      </Card>
    );
  }
  const { actual, expected, pace, items } = progress.data;
  const tone = paceTone(pace);
  return (
    <Card className="gap-4">
      <Text variant="label">{t("goals.progress")}</Text>
      <View className="flex-row flex-wrap gap-6">
        <View className="gap-1">
          <Text variant="heading">{t("goals.actual", { percent: percent(actual) })}</Text>
          <Text variant="caption" tone="muted">
            {t("goals.actualHint")}
          </Text>
        </View>
        <View className="gap-1">
          <Text variant="heading" tone={TONE[tone]}>
            {t("goals.pace", { percent: percent(pace) })}
          </Text>
          <Text variant="caption" tone="muted">
            {t(`goals.paceTone.${tone}`)} ·{" "}
            {t("goals.expectedToDate", { percent: percent(expected) })}
          </Text>
        </View>
      </View>
      <Bar share={actual} marker={expected} />
      <View className="gap-3">
        <Text variant="label">{t("goals.breakdown")}</Text>
        {items.length === 0 ? (
          <Text tone="muted">{t("goals.noBreakdown")}</Text>
        ) : (
          items.map((part) => <BreakdownRow key={part.item.id} part={part} />)
        )}
      </View>
    </Card>
  );
}
