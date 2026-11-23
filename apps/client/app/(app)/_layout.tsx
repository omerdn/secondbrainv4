import { Slot } from "expo-router";

import { SessionGate, useSession, VerifyEmailBanner } from "@/features/auth";
import { AppShell } from "@/features/shell";
import { PinnedViews } from "@/features/views";

/** Every signed-in screen: session gate (T0.4.5) + app shell (T0.4.6). */
export default function AppLayout() {
  return (
    <SessionGate require="signedIn">
      <Shell />
    </SessionGate>
  );
}

function Shell() {
  const { state } = useSession();
  return (
    <AppShell
      account={state.status === "signedIn" ? state.user.display_name : null}
      sidebarExtra={<PinnedViews />}
    >
      <VerifyEmailBanner />
      <Slot />
    </AppShell>
  );
}
