import { Button } from "@secondbrain/ui";
import { Text, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

import { SessionGate, useSession } from "@/features/auth";
import { APP_NAME, platformLabel } from "@/lib/app-info";

/**
 * Placeholder home screen (T0.4.1), now behind the session gate (T0.4.5). Real screens arrive
 * with the app shell (T0.4.6). The team badge shows the team color variable (T0.4.2).
 */
export default function Home() {
  return (
    <SessionGate require="signedIn">
      <SignedInHome />
    </SessionGate>
  );
}

function SignedInHome() {
  const { state, signOut } = useSession();
  return (
    <SafeAreaView className="flex-1 bg-background">
      <View className="flex-1 items-center justify-center gap-2 px-4">
        <Text accessibilityRole="header" className="text-3xl font-semibold text-foreground">
          {APP_NAME}
        </Text>
        {state.status === "signedIn" ? (
          <Text className="text-base text-foreground">{state.user.display_name}</Text>
        ) : null}
        <Text className="text-base text-muted">{platformLabel()}</Text>
        <View className="mt-4 rounded-full bg-team px-3 py-1">
          <Text className="text-sm font-medium text-team-foreground">Team</Text>
        </View>
        <Button title="Çıkış yap" variant="ghost" className="mt-4" onPress={() => void signOut()} />
      </View>
    </SafeAreaView>
  );
}
