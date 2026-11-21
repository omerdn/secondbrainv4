import { Card } from "@secondbrain/ui";
import { usePathname } from "expo-router";

import { isActive, more, moreItems } from "./nav";
import { NavItem } from "./NavItem";
import { Screen } from "./Screen";

/** Phones: the destinations that have no bottom tab of their own. */
export function MoreScreen() {
  const pathname = usePathname();
  return (
    <Screen title={more.label}>
      <Card className="gap-0 py-1">
        {moreItems.map((d) => (
          <NavItem key={d.key} destination={d} active={isActive(pathname, d.href)} variant="list" />
        ))}
      </Card>
    </Screen>
  );
}
