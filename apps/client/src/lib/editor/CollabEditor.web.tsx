import Collaboration from "@tiptap/extension-collaboration";
import CollaborationCaret from "@tiptap/extension-collaboration-caret";
import { EditorContent, useEditor } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useEffect, useMemo, useState } from "react";
import { WebsocketProvider } from "y-websocket";
import * as Y from "yjs";

import { type CollabEditorProps, collabServerUrl, cursorColor, PAGE_FIELD } from "./collab";

/** Live cursor styles (TipTap ships none) and theme colours for the editable area. */
const EDITOR_CSS = `
.collab-editor .ProseMirror { outline: none; min-height: 200px; color: rgb(var(--color-foreground)); }
.collab-editor .ProseMirror p { margin: 0 0 8px; }
.collaboration-carets__caret {
  position: relative; margin-left: -1px; margin-right: -1px; border-left: 1px solid;
  border-right: 1px solid; word-break: normal; pointer-events: none;
}
.collaboration-carets__label {
  position: absolute; top: -1.4em; left: -1px; padding: 0 4px; border-radius: 3px 3px 3px 0;
  font-size: 11px; font-weight: 600; line-height: normal; white-space: nowrap;
  color: #fff; user-select: none; pointer-events: none;
}
`;

/**
 * Web editor: TipTap on the DOM, synchronised with Yjs over y-websocket. The browser sends
 * the session cookie on the handshake by itself (the server checks the Origin).
 */
export function CollabEditor({ pageId, user }: CollabEditorProps) {
  const doc = useMemo(() => new Y.Doc(), []);
  const provider = useMemo(
    () => new WebsocketProvider(collabServerUrl(), pageId, doc),
    [doc, pageId],
  );
  const [status, setStatus] = useState("connecting");

  useEffect(() => {
    const onStatus = ({ status: next }: { status: string }) => setStatus(next);
    provider.on("status", onStatus);
    return () => {
      provider.off("status", onStatus);
      provider.destroy();
      doc.destroy();
    };
  }, [doc, provider]);

  const editor = useEditor({
    extensions: [
      // Yjs keeps the history; the editor's own undo stack would fight it.
      StarterKit.configure({ undoRedo: false }),
      Collaboration.configure({ document: doc, field: PAGE_FIELD }),
      CollaborationCaret.configure({
        provider,
        user: { name: user.name, color: cursorColor(user.id) },
      }),
    ],
  });

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, flex: 1 }}>
      <span
        data-testid="collab-status"
        style={{ fontSize: 12, opacity: 0.7, color: "rgb(var(--color-foreground))" }}
      >
        {status}
      </span>
      <style>{EDITOR_CSS}</style>
      <div
        data-testid="collab-editor"
        className="collab-editor"
        style={{
          border: "1px solid rgba(127,127,127,0.4)",
          borderRadius: 8,
          padding: 12,
          minHeight: 240,
        }}
      >
        <EditorContent editor={editor} />
      </div>
    </div>
  );
}
