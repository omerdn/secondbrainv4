/**
 * Web Push in the browser (PRD BL1, BL3, T4.2.4): the service worker at `/sw.js` and this
 * browser's push subscription. Only the web build in a real browser can push; the desktop
 * shell and the native apps get theirs elsewhere (T4.1.x, T4.2.3).
 *
 * iOS Safari pushes only to a web app added to the home screen.
 */
import type { PushSubscriptionRequest } from "@secondbrain/api-types";
import { Platform } from "react-native";

import { isTauri } from "./platform";

export const SERVICE_WORKER_URL = "/sw.js";

/** Where this browser stands. */
export type WebPushState = "unsupported" | "denied" | "off" | "on";

/** Whether this runtime can receive Web Push at all. */
export function webPushSupported(scope: typeof globalThis = globalThis): boolean {
  return (
    Platform.OS === "web" &&
    !isTauri(scope) &&
    typeof scope.navigator !== "undefined" &&
    "serviceWorker" in scope.navigator &&
    "PushManager" in scope &&
    "Notification" in scope
  );
}

/** A base64url VAPID key as the bytes `applicationServerKey` wants. */
export function keyBytes(base64url: string): Uint8Array<ArrayBuffer> {
  const base64 = base64url.replace(/-/gu, "+").replace(/_/gu, "/");
  const padded = base64 + "=".repeat((4 - (base64.length % 4)) % 4);
  const binary = atob(padded);
  const bytes = new Uint8Array(new ArrayBuffer(binary.length));
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

/** The request body of a browser subscription, or `null` if it has no keys. */
export function toRequest(subscription: PushSubscription): PushSubscriptionRequest | null {
  const json = subscription.toJSON();
  const p256dh = json.keys?.p256dh;
  const auth = json.keys?.auth;
  if (!json.endpoint || !p256dh || !auth) {
    return null;
  }
  return { endpoint: json.endpoint, keys: { p256dh, auth } };
}

async function registration(): Promise<ServiceWorkerRegistration | undefined> {
  return navigator.serviceWorker.getRegistration(SERVICE_WORKER_URL);
}

/** This browser's subscription, if it has one. */
export async function currentSubscription(): Promise<PushSubscription | null> {
  if (!webPushSupported()) {
    return null;
  }
  const reg = await registration();
  return (await reg?.pushManager.getSubscription()) ?? null;
}

/** Where this browser stands now. */
export async function webPushState(): Promise<WebPushState> {
  if (!webPushSupported()) {
    return "unsupported";
  }
  if (Notification.permission === "denied") {
    return "denied";
  }
  return (await currentSubscription()) ? "on" : "off";
}

/**
 * Asks for permission, registers the service worker and subscribes with the server's key.
 * `null` when the user did not allow notifications.
 */
export async function subscribe(publicKey: string): Promise<PushSubscriptionRequest | null> {
  const permission = await Notification.requestPermission();
  if (permission !== "granted") {
    return null;
  }
  await navigator.serviceWorker.register(SERVICE_WORKER_URL, { scope: "/" });
  const reg = await navigator.serviceWorker.ready;
  const existing = await reg.pushManager.getSubscription();
  const subscription =
    existing ??
    (await reg.pushManager.subscribe({
      userVisibleOnly: true,
      applicationServerKey: keyBytes(publicKey),
    }));
  return toRequest(subscription);
}

/** Unsubscribes this browser; its endpoint, so the server can forget it too. */
export async function unsubscribe(): Promise<string | null> {
  const subscription = await currentSubscription();
  if (!subscription) {
    return null;
  }
  const endpoint = subscription.endpoint;
  await subscription.unsubscribe();
  return endpoint;
}
