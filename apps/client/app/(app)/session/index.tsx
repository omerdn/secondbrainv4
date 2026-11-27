import { useLocalSearchParams } from "expo-router";

import { SessionScreen } from "@/features/sessions";

export default function Page() {
  const { shared } = useLocalSearchParams<{ shared?: string }>();
  return <SessionScreen sharedId={shared} />;
}
