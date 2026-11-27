import { Slot } from "expo-router";
import { useState } from "react";

import { SessionGate, useSession, VerifyEmailBanner } from "@/features/auth";
import { useUnreadCount } from "@/features/notifications";
import { SearchSheet, useSearchShortcut } from "@/features/search";
import { AppShell } from "@/features/shell";
import { PinnedViews } from "@/features/views";

/** Every signed-in screen: session gate (T0.4.5) + app shell (T0.4.6) + global search (T2.7.6). */
export default function AppLayout() {
  return (
    <SessionGate require="signedIn">
      <Shell />
    </SessionGate>
  );
}

function Shell() {
  const { state } = useSession();
  const [searching, setSearching] = useState(false);
  const openSearch = () => {
    setSearching(true);
  };
  useSearchShortcut(openSearch);
  const unread = useUnreadCount();
  return (
    <AppShell
      account={state.status === "signedIn" ? state.user.display_name : null}
      sidebarExtra={<PinnedViews />}
      onSearch={openSearch}
      badges={{ notifications: unread }}
    >
      <VerifyEmailBanner />
      <Slot />
      <SearchSheet
        visible={searching}
        onClose={() => {
          setSearching(false);
        }}
      />
    </AppShell>
  );
}
