/** How long an item stays in the trash before it is purged (PRD N4). */
export const TRASH_RETENTION_DAYS = 30;

const DAY_MS = 24 * 60 * 60 * 1000;

/**
 * Whole days left before an item trashed at `deletedAt` is purged, rounded up so that "1" means
 * "less than a day". Never negative: the daily purge may run a little after the deadline.
 */
export function trashDaysLeft(deletedAt: string, now: Date): number {
  const purgeAt = new Date(deletedAt).getTime() + TRASH_RETENTION_DAYS * DAY_MS;
  return Math.max(0, Math.ceil((purgeAt - now.getTime()) / DAY_MS));
}
