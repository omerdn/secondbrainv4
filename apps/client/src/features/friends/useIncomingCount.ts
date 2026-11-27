import { useApi } from "@secondbrain/api-client/react";
import { useQuery } from "@tanstack/react-query";

/** The query key prefix of everything on the friends screen. */
export const FRIENDS_KEY = ["friends"] as const;

/** Friend requests waiting for the caller, for the navigation badge (PRD AK1). */
export function useIncomingCount(): number {
  const api = useApi();
  const incoming = useQuery({
    queryKey: [...FRIENDS_KEY, "incoming"],
    queryFn: () => api.friends.incoming(),
  });
  return incoming.data?.length ?? 0;
}
