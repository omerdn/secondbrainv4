import { Button, Card, Text } from "@secondbrain/ui";

import { useSession } from "@/features/auth";
import { Screen } from "@/features/shell";

/** Settings (more arrive with profile editing); for now the account and sign-out. */
export default function Settings() {
  const { state, signOut } = useSession();
  return (
    <Screen title="Ayarlar">
      {state.status === "signedIn" ? (
        <Card className="gap-1">
          <Text variant="heading">{state.user.display_name}</Text>
          <Text tone="muted">{state.user.email}</Text>
        </Card>
      ) : null}
      <Button title="Çıkış yap" variant="secondary" onPress={() => void signOut()} />
    </Screen>
  );
}
