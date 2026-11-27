import type { Activity, ActivityReaction } from "@secondbrain/api-types";

/** The emoji the server accepts as reactions (PRD AK4); the same order. */
export const REACTIONS = ["👏", "🔥", "💪", "🎉", "❤️", "👍"] as const;

/** Completions by friends, newest first, that the reader can react to (AK4). */
export function friendCompletions(
  feed: readonly Activity[],
  friendIds: ReadonlySet<string>,
  me: string | null,
): Activity[] {
  return feed.filter(
    (event) =>
      event.kind === "completed" &&
      event.actor_id !== null &&
      event.actor_id !== me &&
      friendIds.has(event.actor_id),
  );
}

/** Pressing `emoji`: takes the reader's reaction back if it was this one, otherwise sets it. */
export function reactionAction(
  reactions: readonly ActivityReaction[],
  emoji: string,
): "react" | "unreact" {
  return reactions.some((r) => r.emoji === emoji && r.mine) ? "unreact" : "react";
}

/** How many reacted with `emoji`. */
export function reactionCount(reactions: readonly ActivityReaction[], emoji: string): number {
  return reactions.find((r) => r.emoji === emoji)?.count ?? 0;
}
