/** Connection layer (ARCHITECTURE §10.2, PRD BP1–BP5): state machine and its provider. */
export { ConnectivityGate } from "./ConnectivityGate";
export {
  type Connectivity,
  ConnectivityProvider,
  type LinkOpener,
  type NetworkSource,
  useConnectivity,
} from "./ConnectivityProvider";
export type { Link, LinkHandlers } from "./link";
export type { ConnectivityState } from "./machine";
export { useOnConnected } from "./useOnConnected";
