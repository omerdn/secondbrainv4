import { useEffect, useRef } from "react";

import { useConnectivity } from "./ConnectivityProvider";

/**
 * Calls `callback` each time the connection comes up (state becomes `online`), with the
 * latest `callback`. For work that failed for lack of connection, e.g. restoring the session.
 */
export function useOnConnected(callback: () => void): void {
  const { state } = useConnectivity();
  const latest = useRef(callback);
  latest.current = callback;
  useEffect(() => {
    if (state.kind === "online") latest.current();
  }, [state.kind]);
}
