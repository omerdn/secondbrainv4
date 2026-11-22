import { Platform } from "react-native";

/**
 * True inside the Tauri desktop shell (ADR-0020): the packaged web build runs in a webview
 * that exposes `__TAURI_INTERNALS__`.
 */
export function isTauri(scope: object = globalThis): boolean {
  return "__TAURI_INTERNALS__" in scope;
}

/** How the session travels (ARCHITECTURE §15): browser cookie or bearer token. */
export type Transport = "cookie" | "bearer";

/**
 * Browsers use the `HttpOnly` cookie. Native apps and the desktop shell send a bearer token:
 * the desktop app runs on its own origin, where the API cookie is cross-site and not sent
 * (ADR-0020).
 */
export function sessionTransport(os: string = Platform.OS, tauri: boolean = isTauri()): Transport {
  return os === "web" && !tauri ? "cookie" : "bearer";
}
