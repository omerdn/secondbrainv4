import { Text } from "@secondbrain/ui";
import { Redirect, useLocalSearchParams } from "expo-router";
import { View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

import { useSession } from "@/features/auth";
import { CollabEditor } from "@/lib/editor/CollabEditor";

/** A fixed page id so two browsers meet in the same room unless `?page=` says otherwise. */
const DEMO_PAGE = "01a20000-0000-7000-8000-000000000001";

/**
 * Spike T0.6.2 (ADR-0019): two clients edit the same page through `/collab/{page_id}`.
 * Development only; needs a signed-in user who can see the page.
 */
export default function EditorSpike() {
  const { state } = useSession();
  const { page } = useLocalSearchParams<{ page?: string }>();
  if (!__DEV__) {
    return <Redirect href="/" />;
  }
  if (state.status === "signedOut") {
    return <Redirect href="/login" />;
  }
  if (state.status !== "signedIn") {
    return null;
  }
  return (
    <SafeAreaView className="flex-1 bg-background">
      <View className="flex-1 gap-3 p-4">
        <Text variant="heading">Ortak düzenleme spike</Text>
        <Text tone="muted">Sayfa: {page ?? DEMO_PAGE}</Text>
        <CollabEditor
          pageId={page ?? DEMO_PAGE}
          user={{ id: state.user.id, name: state.user.display_name }}
        />
      </View>
    </SafeAreaView>
  );
}
