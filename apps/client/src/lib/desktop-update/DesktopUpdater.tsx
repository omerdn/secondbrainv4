import { Card, Text } from "@secondbrain/ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { ActivityIndicator, View } from "react-native";

import { isTauri } from "@/lib/platform";

import { runDesktopUpdate, type UpdatePhase, type UpdaterDeps } from "./update";
import { desktopUpdater } from "./updater";

/**
 * Checks for a desktop update once per launch and covers the app while it downloads and
 * restarts. Renders nothing outside the desktop shell or when the app is current.
 */
export function DesktopUpdater({
  updater = isTauri() ? desktopUpdater : null,
}: {
  updater?: UpdaterDeps | null;
}) {
  const { t } = useTranslation();
  const [phase, setPhase] = useState<UpdatePhase>({ kind: "idle" });

  useEffect(() => {
    if (updater === null) return;
    void runDesktopUpdate(updater, setPhase);
  }, [updater]);

  if (phase.kind === "idle") return null;
  const message =
    phase.kind === "restarting"
      ? t("desktopUpdate.restarting")
      : t("desktopUpdate.downloading", { version: phase.version });
  return (
    <View
      className="absolute inset-0 items-center justify-center bg-background/90 px-4"
      accessibilityViewIsModal
      accessibilityLiveRegion="polite"
    >
      <Card className="items-center gap-3">
        <ActivityIndicator />
        <Text variant="heading">{message}</Text>
        {phase.kind === "downloading" && phase.percent !== null ? (
          <Text tone="muted">{t("desktopUpdate.progress", { percent: phase.percent })}</Text>
        ) : null}
      </Card>
    </View>
  );
}
