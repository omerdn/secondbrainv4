import { CollabEditor } from "@/lib/editor/CollabEditor";

/** Web: the collaborative TipTap editor (T2.5.7, ADR-0019). */
export function PageEditor({
  pageId,
  user,
}: {
  pageId: string;
  user: { id: string; name: string };
}) {
  return <CollabEditor pageId={pageId} user={user} />;
}
