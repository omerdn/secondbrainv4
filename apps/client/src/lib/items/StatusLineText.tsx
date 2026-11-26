import type { StatusLine } from "@secondbrain/api-types";
import { Text } from "@secondbrain/ui";
import { useTranslation } from "react-i18next";

import { daysBetween, localToday } from "./statusLines";

/** "Barre'da kaldım · 3 gün önce" (PRD S3). */
export function StatusLineText({
  line,
  today = localToday(),
}: {
  line: StatusLine;
  today?: string;
}) {
  const { t } = useTranslation();
  const days = line.date === null ? null : daysBetween(line.date, today);
  const when =
    days === null
      ? null
      : days <= 0
        ? t("items.statusLine.today")
        : days === 1
          ? t("items.statusLine.yesterday")
          : t("items.statusLine.daysAgo", { count: days });
  return (
    <Text variant="label" className="font-normal" numberOfLines={2} testID="status-line">
      {when === null ? line.text : `${line.text} · ${when}`}
    </Text>
  );
}
