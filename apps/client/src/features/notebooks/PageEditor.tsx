import { Card, Text } from "@secondbrain/ui";
import { useTranslation } from "react-i18next";

/** Mobile and desktop shells: the page editor comes with T4.2.2 (ADR-0024). */
export function PageEditor(_props: { pageId: string; user: { id: string; name: string } }) {
  const { t } = useTranslation();
  return (
    <Card>
      <Text tone="muted">{t("notebooks.editorSoon")}</Text>
    </Card>
  );
}
