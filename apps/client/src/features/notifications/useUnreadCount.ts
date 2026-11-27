import { useApi } from "@secondbrain/api-client/react";
import { useQuery } from "@tanstack/react-query";

/** The query key of the center; `notification.new` events invalidate it (live, T0.5.5). */
export const NOTIFICATIONS_KEY = ["notifications"] as const;

/** Unread notifications, for the navigation badge (PRD BL1); 0 until known. */
export function useUnreadCount(): number {
  const api = useApi();
  const list = useQuery({
    queryKey: [...NOTIFICATIONS_KEY, "list"],
    queryFn: () => api.notifications.list(),
  });
  return list.data?.unread ?? 0;
}
