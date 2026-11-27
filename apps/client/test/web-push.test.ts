import { keyBytes, toRequest, webPushSupported } from "@/lib/web-push";

describe("web push (T4.2.4)", () => {
  it("decodes a base64url VAPID key", () => {
    expect(Array.from(keyBytes("AQID_-8"))).toEqual([1, 2, 3, 255, 239]);
  });

  it("turns a browser subscription into the request body", () => {
    const subscription = {
      toJSON: () => ({
        endpoint: "https://fcm.googleapis.com/fcm/send/x",
        expirationTime: null,
        keys: { p256dh: "pk", auth: "au" },
      }),
    } as unknown as PushSubscription;
    expect(toRequest(subscription)).toEqual({
      endpoint: "https://fcm.googleapis.com/fcm/send/x",
      keys: { p256dh: "pk", auth: "au" },
    });
    const keyless = { toJSON: () => ({ endpoint: "https://x" }) } as unknown as PushSubscription;
    expect(toRequest(keyless)).toBeNull();
  });

  it("is unsupported outside a browser and in the desktop shell", () => {
    expect(webPushSupported({} as typeof globalThis)).toBe(false);
    const browser = {
      navigator: { serviceWorker: {} },
      PushManager: class {},
      Notification: class {},
    } as unknown as typeof globalThis;
    const shell = { ...browser, __TAURI_INTERNALS__: {} } as unknown as typeof globalThis;
    expect(webPushSupported(shell)).toBe(false);
  });
});
