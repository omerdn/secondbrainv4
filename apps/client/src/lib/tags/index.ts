import type { Tag } from "@secondbrain/api-types";
import { useApi } from "@secondbrain/api-client/react";
import { useQuery } from "@tanstack/react-query";

/**
 * The color to draw each tag in: a team tag takes its team's color, a personal tag none (PRD E3,
 * T5.1.4). Tags carry only `team_id`; the colors come from the caller's team list, which is
 * fetched only when one of `tags` belongs to a team.
 */
export function useTagColors(tags: readonly Tag[] | undefined): (tag: Tag) => string | null {
  const api = useApi();
  const hasTeamTags = tags?.some((tag) => tag.team_id !== null) ?? false;
  const teams = useQuery({ queryKey: ["teams"], queryFn: api.teams.list, enabled: hasTeamTags });
  const colors = new Map(teams.data?.map((team) => [team.id, team.color]));
  return (tag) => (tag.team_id === null ? null : (colors.get(tag.team_id) ?? null));
}
