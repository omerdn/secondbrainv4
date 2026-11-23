import { Text } from "@secondbrain/ui";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { barShare, type DayValue, formatAmount } from "./model";

/** Not a spacing token: the design scale has no step this tall. */
const CHART_HEIGHT = 128;

/** Day-by-day bars built from plain views, so it works on every platform (PRD K3, ADR-0012). */
export function HistoryChart({ series, unit }: { series: readonly DayValue[]; unit: string }) {
  const { t, i18n } = useTranslation();
  const first = series[0];
  const last = series[series.length - 1];
  if (series.every((point) => point.value === null)) {
    return <Text tone="muted">{t("trackers.noHistory")}</Text>;
  }
  return (
    <View className="gap-1">
      <View className="flex-row items-end gap-1" style={{ height: CHART_HEIGHT }}>
        {series.map((point) => {
          const label =
            point.value === null
              ? t("trackers.chartEmptyDay", { day: point.day })
              : t("trackers.chartDay", {
                  day: point.day,
                  value: formatAmount(point.value, i18n.language),
                  unit,
                });
          const share = barShare(point.value, series);
          return (
            <View
              key={point.day}
              accessible
              accessibilityRole="image"
              accessibilityLabel={label}
              className="h-full flex-1 justify-end"
            >
              <View
                className={point.value === null ? "rounded-sm bg-border" : "rounded-sm bg-primary"}
                style={{ height: point.value === null ? 2 : `${Math.max(share * 100, 2)}%` }}
              />
            </View>
          );
        })}
      </View>
      <View className="flex-row justify-between">
        <Text variant="caption" tone="muted">
          {first?.day}
        </Text>
        <Text variant="caption" tone="muted">
          {last?.day}
        </Text>
      </View>
    </View>
  );
}
