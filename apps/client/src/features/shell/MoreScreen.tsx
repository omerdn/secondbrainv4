import { Card } from "@secondbrain/ui";
import { usePathname } from "expo-router";
import { useTranslation } from "react-i18next";

import { isActive, moreItems } from "./nav";
import { NavItem } from "./NavItem";
import { Screen } from "./Screen";

/** Phones: the destinations that have no bottom tab of their own. */
export function MoreScreen() {
  const pathname = usePathname();
  const { t } = useTranslation();
  return (
    <Screen title={t("nav.more")}>
      <Card className="gap-0 py-1">
        {moreItems.map((d) => (
          <NavItem key={d.key} destination={d} active={isActive(pathname, d.href)} variant="list" />
        ))}
      </Card>
    </Screen>
  );
}
