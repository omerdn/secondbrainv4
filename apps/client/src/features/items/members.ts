import type { Friend, InvitationRole, ItemMembers } from "@secondbrain/api-types";

/** The roles an invitation may offer on an item of `kind` (mirrors the server's `role_allowed`). */
export function rolesFor(kind: string): InvitationRole[] {
  if (kind === "page") return [];
  return kind === "notebook" ? ["editor", "viewer"] : ["assignee", "watcher"];
}

/** Friends who are neither members nor invited yet: whom the owner can still invite. */
export function invitable(friends: readonly Friend[], members: ItemMembers): Friend[] {
  const taken = new Set([
    ...members.members.map((m) => m.user_id),
    ...members.pending.map((p) => p.user_id),
  ]);
  return friends.filter((friend) => !taken.has(friend.user_id));
}
