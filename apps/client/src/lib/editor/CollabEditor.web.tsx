import Collaboration from "@tiptap/extension-collaboration";
import CollaborationCaret from "@tiptap/extension-collaboration-caret";
import { TaskItem, TaskList } from "@tiptap/extension-list";
import { TableKit } from "@tiptap/extension-table";
import { type Editor, EditorContent, Extension, useEditor, useEditorState } from "@tiptap/react";
import StarterKit from "@tiptap/starter-kit";
import { useEffect, useMemo, useState } from "react";
import { useTranslation } from "react-i18next";
import { WebsocketProvider } from "y-websocket";
import * as Y from "yjs";

import {
  type CollabEditorProps,
  type CollabStatus,
  collabServerUrl,
  cursorColor,
  PAGE_FIELD,
  SERVER_LINE_ATTRIBUTES,
} from "./collab";

/** Theme colours for the editable area, the Y4 blocks and live cursors (TipTap ships none). */
const EDITOR_CSS = `
.collab-editor .ProseMirror { outline: none; min-height: 240px; color: rgb(var(--color-foreground)); }
.collab-editor .ProseMirror p { margin: 0 0 8px; }
.collab-editor .ProseMirror h1 { font-size: 1.6em; font-weight: 700; margin: 12px 0 8px; }
.collab-editor .ProseMirror h2 { font-size: 1.35em; font-weight: 700; margin: 12px 0 8px; }
.collab-editor .ProseMirror h3 { font-size: 1.15em; font-weight: 700; margin: 10px 0 6px; }
.collab-editor .ProseMirror ul { list-style: disc; padding-left: 1.4em; }
.collab-editor .ProseMirror ol { list-style: decimal; padding-left: 1.4em; }
.collab-editor .ProseMirror ul[data-type="taskList"] { list-style: none; padding-left: 0.2em; }
.collab-editor .ProseMirror ul[data-type="taskList"] li { display: flex; gap: 8px; align-items: flex-start; }
.collab-editor .ProseMirror ul[data-type="taskList"] li > div { flex: 1; }
.collab-editor .ProseMirror blockquote { border-left: 3px solid rgba(127,127,127,0.5); padding-left: 12px; opacity: 0.85; }
.collab-editor .ProseMirror pre { background: rgba(127,127,127,0.15); border-radius: 6px; padding: 10px 12px; font-family: ui-monospace, monospace; white-space: pre-wrap; }
.collab-editor .ProseMirror code { font-family: ui-monospace, monospace; }
.collab-editor .ProseMirror table { border-collapse: collapse; margin: 8px 0; width: 100%; }
.collab-editor .ProseMirror td, .collab-editor .ProseMirror th { border: 1px solid rgba(127,127,127,0.45); padding: 4px 8px; vertical-align: top; min-width: 60px; }
.collab-editor .ProseMirror th { font-weight: 600; background: rgba(127,127,127,0.12); }
.collab-editor[data-readonly="true"] .ProseMirror { opacity: 0.6; }
.collab-toolbar button { border: 1px solid rgba(127,127,127,0.4); border-radius: 6px; padding: 2px 8px; font-size: 13px; color: rgb(var(--color-foreground)); background: transparent; cursor: pointer; }
.collab-toolbar button[aria-pressed="true"] { background: rgba(127,127,127,0.25); }
.collab-toolbar button:disabled { opacity: 0.4; cursor: default; }
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
 * Keeps the attributes the server writes on process page lines (T3: `blockId`, `date`), so
 * editing a line never drops its link to the entry.
 */
const ServerLineAttributes = Extension.create({
  name: "serverLineAttributes",
  addGlobalAttributes() {
    return [
      {
        types: ["paragraph"],
        attributes: Object.fromEntries(
          SERVER_LINE_ATTRIBUTES.map((name) => [
            name,
            {
              default: null,
              parseHTML: (element: HTMLElement) => element.getAttribute(`data-${name}`),
              renderHTML: (attributes: Record<string, unknown>) => {
                const value = attributes[name];
                return typeof value === "string" ? { [`data-${name}`]: value } : {};
              },
            },
          ]),
        ),
      },
    ];
  },
});

type ActionKey =
  | "h1"
  | "h2"
  | "bold"
  | "italic"
  | "bulletList"
  | "orderedList"
  | "taskList"
  | "codeBlock"
  | "blockquote"
  | "table"
  | "addRow"
  | "addColumn"
  | "deleteTable";

type Action = {
  key: ActionKey;
  active?: (editor: Editor) => boolean;
  run: (editor: Editor) => void;
};

/** The Y4 formats: headings, lists, to-dos, code, quotes and tables. */
const ACTIONS: Action[] = [
  {
    key: "h1",
    active: (e) => e.isActive("heading", { level: 1 }),
    run: (e) => e.chain().focus().toggleHeading({ level: 1 }).run(),
  },
  {
    key: "h2",
    active: (e) => e.isActive("heading", { level: 2 }),
    run: (e) => e.chain().focus().toggleHeading({ level: 2 }).run(),
  },
  {
    key: "bold",
    active: (e) => e.isActive("bold"),
    run: (e) => e.chain().focus().toggleBold().run(),
  },
  {
    key: "italic",
    active: (e) => e.isActive("italic"),
    run: (e) => e.chain().focus().toggleItalic().run(),
  },
  {
    key: "bulletList",
    active: (e) => e.isActive("bulletList"),
    run: (e) => e.chain().focus().toggleBulletList().run(),
  },
  {
    key: "orderedList",
    active: (e) => e.isActive("orderedList"),
    run: (e) => e.chain().focus().toggleOrderedList().run(),
  },
  {
    key: "taskList",
    active: (e) => e.isActive("taskList"),
    run: (e) => e.chain().focus().toggleTaskList().run(),
  },
  {
    key: "codeBlock",
    active: (e) => e.isActive("codeBlock"),
    run: (e) => e.chain().focus().toggleCodeBlock().run(),
  },
  {
    key: "blockquote",
    active: (e) => e.isActive("blockquote"),
    run: (e) => e.chain().focus().toggleBlockquote().run(),
  },
  {
    key: "table",
    active: (e) => e.isActive("table"),
    run: (e) => e.chain().focus().insertTable({ rows: 3, cols: 3, withHeaderRow: true }).run(),
  },
  { key: "addRow", run: (e) => e.chain().focus().addRowAfter().run() },
  { key: "addColumn", run: (e) => e.chain().focus().addColumnAfter().run() },
  { key: "deleteTable", run: (e) => e.chain().focus().deleteTable().run() },
];

const TABLE_ONLY = new Set<ActionKey>(["addRow", "addColumn", "deleteTable"]);

function Toolbar({ editor, editable }: { editor: Editor; editable: boolean }) {
  const { t } = useTranslation();
  const state = useEditorState({
    editor,
    selector: ({ editor: e }) => ({
      active: Object.fromEntries(ACTIONS.map((a) => [a.key, a.active?.(e) ?? false])),
      inTable: e.isActive("table"),
    }),
  });
  return (
    <div
      className="collab-toolbar"
      role="toolbar"
      aria-label={t("editor.toolbar")}
      style={{ display: "flex", flexWrap: "wrap", gap: 6 }}
    >
      {ACTIONS.filter((a) => !TABLE_ONLY.has(a.key) || state.inTable).map((action) => (
        <button
          key={action.key}
          type="button"
          aria-pressed={action.active === undefined ? undefined : state.active[action.key]}
          disabled={!editable}
          onMouseDown={(event) => {
            // Keep the selection in the editor.
            event.preventDefault();
          }}
          onClick={() => {
            action.run(editor);
          }}
        >
          {t(`editor.actions.${action.key}`)}
        </button>
      ))}
    </div>
  );
}

/**
 * Web editor (ADR-0019): TipTap on the DOM, synchronised with Yjs over y-websocket. The browser
 * sends the session cookie on the handshake by itself (the server checks the Origin). Online
 * only (ADR-0003): while the connection is down the editor is read-only; y-websocket reconnects
 * and resynchronises, so anything typed before the drop is sent once it is back.
 */
export function CollabEditor({ pageId, user, readOnly = false }: CollabEditorProps) {
  const { t } = useTranslation();
  const doc = useMemo(() => new Y.Doc(), []);
  const provider = useMemo(
    () => new WebsocketProvider(collabServerUrl(), pageId, doc),
    [doc, pageId],
  );
  const [status, setStatus] = useState<CollabStatus>("connecting");
  const [synced, setSynced] = useState(false);

  useEffect(() => {
    const onStatus = ({ status: next }: { status: CollabStatus }) => {
      setStatus(next);
    };
    const onSync = (isSynced: boolean) => {
      setSynced(isSynced);
    };
    // A dead network can leave the socket looking open for a while; go read-only at once and
    // resynchronise when the browser is back online.
    const onOffline = () => {
      provider.disconnect();
    };
    const onOnline = () => {
      provider.connect();
    };
    provider.on("status", onStatus);
    provider.on("sync", onSync);
    window.addEventListener("offline", onOffline);
    window.addEventListener("online", onOnline);
    return () => {
      window.removeEventListener("offline", onOffline);
      window.removeEventListener("online", onOnline);
      provider.off("status", onStatus);
      provider.off("sync", onSync);
      provider.destroy();
      doc.destroy();
    };
  }, [doc, provider]);

  const editable = !readOnly && status === "connected" && synced;
  const editor = useEditor({
    editable: false,
    extensions: [
      // Yjs keeps the history; the editor's own undo stack would fight it.
      StarterKit.configure({ undoRedo: false }),
      TaskList,
      TaskItem.configure({ nested: true }),
      TableKit.configure({ table: { resizable: false } }),
      ServerLineAttributes,
      Collaboration.configure({ document: doc, field: PAGE_FIELD }),
      CollaborationCaret.configure({
        provider,
        user: { name: user.name, color: cursorColor(user.id) },
      }),
    ],
  });

  useEffect(() => {
    editor.setEditable(editable);
  }, [editor, editable]);

  const label =
    status === "connected" && synced
      ? readOnly
        ? t("editor.status.viewOnly")
        : t("editor.status.connected")
      : status === "disconnected"
        ? t("editor.status.disconnected")
        : t("editor.status.connecting");

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 8, flex: 1 }}>
      <style>{EDITOR_CSS}</style>
      <span
        data-testid="collab-status"
        data-status={editable ? "editable" : "readonly"}
        role="status"
        style={{ fontSize: 12, opacity: 0.7, color: "rgb(var(--color-foreground))" }}
      >
        {label}
      </span>
      {readOnly ? null : <Toolbar editor={editor} editable={editable} />}
      <div
        data-testid="collab-editor"
        data-readonly={editable ? "false" : "true"}
        className="collab-editor"
        style={{
          border: "1px solid rgba(127,127,127,0.4)",
          borderRadius: 8,
          padding: 12,
          minHeight: 260,
        }}
      >
        <EditorContent editor={editor} />
      </div>
    </div>
  );
}
