import { Stack } from "expo-router";

import { SessionGate } from "@/features/auth";

/** Sign-in screens; a signed-in user is sent home. */
export default function AuthLayout() {
  return (
    <SessionGate require="signedOut">
      <Stack screenOptions={{ headerShown: false }} />
    </SessionGate>
  );
}
