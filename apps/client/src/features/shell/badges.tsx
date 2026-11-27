import { createContext, useContext } from "react";

import { moreItems, type Section } from "./nav";

/** Counts shown on navigation entries, such as unread notifications (T2.8.2). */
export type NavBadges = Partial<Record<Section, number>>;

export const NavBadgesContext = createContext<NavBadges>({});

/** The count on `key`'s entry; "More" carries the counts of everything under it. */
export function useNavBadge(key: Section): number {
  const badges = useContext(NavBadgesContext);
  if (key === "more") {
    return moreItems.reduce((sum, item) => sum + (badges[item.key] ?? 0), 0);
  }
  return badges[key] ?? 0;
}
