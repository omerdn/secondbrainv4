import {
  BridgeExtension,
  type EditorBridge,
  HistoryBridge,
  RichText,
  TenTapStartKit,
  Toolbar,
  useBridgeState,
  useEditorBridge,
} from "@10play/tentap-editor";
import { Text } from "@secondbrain/ui";
import { useEffect, useMemo, useRef, useState } from "react";
import { KeyboardAvoidingView, Platform, View } from "react-native";

import { credentials } from "@/lib/api";

import { type CollabEditorProps, collabServerUrl, cursorColor } from "./collab";
import { fromBase64, toBase64 } from "./native/base64";
import { editorHtml } from "./native/editorHtml";
import { isToNative, type ToNative, type ToWeb } from "./native/protocol";

/** The real socket for one editor; the WebView only sees relayed frames. */
interface Relay {
  url: string;
  token: string | null;
  /** The WebView's current connection id and its socket (see `native/protocol.ts`). */
  id: string | null;
  socket: WebSocket | null;
  toWeb: ((message: ToWeb) => void) | null;
}

type CollabMembers = { collabStart: (user: { name: string; color: string }) => void };

/**
 * Native editor (spike T0.6.2, ADR-0019): 10tap-editor (TipTap in a WebView) with a custom
 * bundle that adds TipTap Collaboration. React Native opens `/collab/{page_id}` with the bearer
 * token and relays binary frames to and from the WebView's y-websocket.
 */
export function CollabEditor({ pageId, user }: CollabEditorProps) {
  const relay = useRef<Relay>({ url: "", token: null, id: null, socket: null, toWeb: null });
  relay.current.url = `${collabServerUrl()}/${pageId}`;
  relay.current.token = credentials.token;
  const [status, setStatus] = useState("connecting");

  const bridge = useMemo(
    () =>
      new BridgeExtension<object, CollabMembers, ToWeb | ToNative>({
        forceName: "collabRelay",
        extendEditorInstance: (sendBridgeMessage) => {
          relay.current.toWeb = sendBridgeMessage;
          return {
            collabStart: (who) => sendBridgeMessage({ type: "collab-start", payload: who }),
          };
        },
        onEditorMessage: (message) => {
          if (!isToNative(message)) return false;
          const r = relay.current;
          switch (message.type) {
            case "collab-connect": {
              const { id } = message.payload;
              r.socket?.close();
              // React Native's WebSocket accepts headers as a third argument.
              const NativeSocket = WebSocket as unknown as new (
                url: string,
                protocols: string[] | null,
                options: { headers: Record<string, string> },
              ) => WebSocket;
              const socket = new NativeSocket(r.url, null, {
                headers: r.token === null ? {} : { Authorization: `Bearer ${r.token}` },
              });
              socket.binaryType = "arraybuffer";
              socket.onopen = () => {
                setStatus("connected");
                r.toWeb?.({ type: "collab-open", payload: { id } });
              };
              socket.onmessage = (event: MessageEvent<ArrayBuffer>) => {
                r.toWeb?.({
                  type: "collab-recv",
                  payload: { id, data: toBase64(new Uint8Array(event.data)) },
                });
              };
              socket.onclose = () => {
                if (r.socket === socket) {
                  r.socket = null;
                  setStatus("disconnected");
                }
                r.toWeb?.({ type: "collab-closed", payload: { id } });
              };
              r.id = id;
              r.socket = socket;
              return true;
            }
            case "collab-send":
              if (message.payload.id === r.id) r.socket?.send(fromBase64(message.payload.data));
              return true;
            case "collab-disconnect":
              if (message.payload.id === r.id) {
                r.socket?.close();
                r.socket = null;
                r.id = null;
              }
              return true;
            default:
              return false;
          }
        },
      }),
    [],
  );

  const editor = useEditorBridge({
    customSource: editorHtml,
    avoidIosKeyboard: true,
    bridgeExtensions: [...TenTapStartKit.filter((b) => b !== HistoryBridge), bridge],
  }) as EditorBridge & CollabMembers;
  const { isReady } = useBridgeState(editor);

  useEffect(() => {
    if (isReady) editor.collabStart({ name: user.name, color: cursorColor(user.id) });
  }, [editor, isReady, user.id, user.name]);

  useEffect(
    () => () => {
      relay.current.socket?.close();
      relay.current.socket = null;
    },
    [],
  );

  return (
    <View className="flex-1 gap-2">
      <Text tone="muted" testID="collab-status">
        {status}
      </Text>
      <View className="flex-1 overflow-hidden rounded-md border border-border">
        <RichText editor={editor} />
      </View>
      <KeyboardAvoidingView behavior={Platform.OS === "ios" ? "padding" : undefined}>
        <Toolbar editor={editor} />
      </KeyboardAvoidingView>
    </View>
  );
}
