/**
 * The editor page loaded into the native WebView (10tap advanced setup, spike T0.6.2,
 * ADR-0019): TenTap bridges + TipTap Collaboration on a Yjs document. y-websocket's socket is
 * a relay over the bridge; React Native holds the real authenticated connection.
 *
 * Built with `pnpm editor:build` into `src/lib/editor/native/editorHtml.js`.
 */
import {
  BridgeExtension,
  HistoryBridge,
  TenTapStartKit,
  useTenTap,
} from "@10play/tentap-editor/web";
import Collaboration from "@tiptap/extension-collaboration";
import CollaborationCaret from "@tiptap/extension-collaboration-caret";
import { EditorContent } from "@tiptap/react";
import { createRoot } from "react-dom/client";
import { WebsocketProvider } from "y-websocket";
import * as Y from "yjs";

import { fromBase64, toBase64 } from "../src/lib/editor/native/base64";
import type { ToNative, ToWeb } from "../src/lib/editor/native/protocol";

type Post = (message: ToNative) => void;
let post: Post | null = null;

/** Minimal WebSocket stand-in for y-websocket; frames travel over the bridge. */
class RelaySocket {
  static readonly CONNECTING = 0;
  static readonly OPEN = 1;
  static readonly CLOSING = 2;
  static readonly CLOSED = 3;
  static current: RelaySocket | null = null;

  readonly CONNECTING = 0;
  readonly OPEN = 1;
  readonly CLOSING = 2;
  readonly CLOSED = 3;
  /** Distinguishes this connection from earlier ones (see `protocol.ts`). */
  readonly id = `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
  binaryType = "arraybuffer";
  readyState = RelaySocket.CONNECTING;
  onopen: ((event: unknown) => void) | null = null;
  onmessage: ((event: { data: ArrayBuffer }) => void) | null = null;
  onclose: ((event: unknown) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;

  constructor() {
    RelaySocket.current = this;
    post?.({ type: "collab-connect", payload: { id: this.id } });
  }

  send(data: ArrayBuffer | Uint8Array) {
    const bytes = data instanceof Uint8Array ? data : new Uint8Array(data);
    post?.({ type: "collab-send", payload: { id: this.id, data: toBase64(bytes) } });
  }

  close() {
    if (this.readyState === RelaySocket.CLOSED) return;
    this.readyState = RelaySocket.CLOSED;
    post?.({ type: "collab-disconnect", payload: { id: this.id } });
    this.onclose?.({});
  }
}

const doc = new Y.Doc();
// The URL and room are placeholders: native knows the real page and opens the socket.
const provider = new WebsocketProvider("ws://relay", "page", doc, {
  connect: false,
  WebSocketPolyfill: RelaySocket as unknown as typeof WebSocket,
});

const CollabRelayBridge = new BridgeExtension<object, object, ToWeb | ToNative>({
  forceName: "collabRelay",
  onBridgeMessage: (_editor, message, sendMessageBack) => {
    post = sendMessageBack;
    const socket = RelaySocket.current;
    switch (message.type) {
      case "collab-start":
        provider.awareness.setLocalStateField("user", message.payload);
        provider.connect();
        return true;
      case "collab-open":
        if (socket?.id === message.payload.id && socket.readyState === RelaySocket.CONNECTING) {
          socket.readyState = RelaySocket.OPEN;
          socket.onopen?.({});
        }
        return true;
      case "collab-recv":
        if (socket?.id === message.payload.id) {
          socket.onmessage?.({ data: fromBase64(message.payload.data).buffer as ArrayBuffer });
        }
        return true;
      case "collab-closed":
        if (socket?.id === message.payload.id && socket.readyState !== RelaySocket.CLOSED) {
          socket.readyState = RelaySocket.CLOSED;
          socket.onclose?.({});
        }
        return true;
      default:
        return false;
    }
  },
});

function Editor() {
  const editor = useTenTap({
    // Yjs keeps the history; TenTap's undo/redo bridge would fight it.
    bridges: [...TenTapStartKit.filter((bridge) => bridge !== HistoryBridge), CollabRelayBridge],
    tiptapOptions: {
      extensions: [
        Collaboration.configure({ document: doc, field: "content" }),
        CollaborationCaret.configure({ provider, user: { name: "…", color: "#2563eb" } }),
      ],
    },
  });
  return <EditorContent editor={editor} />;
}

const CARET_CSS = `
.collaboration-carets__caret { position: relative; margin: 0 -1px; border-left: 1px solid; border-right: 1px solid; pointer-events: none; }
.collaboration-carets__label { position: absolute; top: -1.4em; left: -1px; padding: 0 4px; border-radius: 3px 3px 3px 0; font: 600 11px -apple-system, sans-serif; white-space: nowrap; color: #fff; pointer-events: none; }
`;
const style = document.createElement("style");
style.textContent = CARET_CSS;
document.head.appendChild(style);

const root = document.getElementById("root");
if (root !== null) createRoot(root).render(<Editor />);
