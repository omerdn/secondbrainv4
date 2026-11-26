import type { SessionGlance } from "@secondbrain/api-types";
import { Card, Text } from "@secondbrain/ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { clockFace, clockOffset, formatClock } from "./model";

/** The open work session at a glance (PRD H6), for the Today screen; pressing it opens it. */
export function SessionGlanceCard({
  glance,
  onPress,
}: {
  glance: SessionGlance;
  onPress: () => void;
}) {
  const { t } = useTranslation();
  const [offset] = useState(() => clockOffset(glance.server_time, Date.now()));
  const [now, setNow] = useState(() => Date.now());
  const running = glance.timer.state === "running";
  useEffect(() => {
    setNow(Date.now());
    if (!running) return undefined;
    const id = setInterval(() => {
      setNow(Date.now());
    }, 1000);
    return () => {
      clearInterval(id);
    };
  }, [running, glance]);
  const { timer } = glance;
  const face = clockFace(timer, now + offset);
  const mode =
    timer.state === "stopped" || timer.mode === null
      ? t("sessions.stopped")
      : t(`sessions.modes.${timer.mode}`);
  return (
    <Card onPress={onPress} accessibilityLabel={t("sessions.glance")}>
      <View className="flex-row items-center justify-between gap-3">
        <View className="gap-0.5">
          <Text variant="caption" tone="muted">
            {t("sessions.glance")}
          </Text>
          <Text>
            {mode}
            {timer.state === "paused" ? ` · ${t("sessions.paused")}` : ""}
          </Text>
        </View>
        {timer.state === "stopped" ? null : (
          <Text variant="heading" className="tabular-nums" testID="session-glance-clock">
            {formatClock(face.ms, face.countdown)}
          </Text>
        )}
      </View>
    </Card>
  );
}
