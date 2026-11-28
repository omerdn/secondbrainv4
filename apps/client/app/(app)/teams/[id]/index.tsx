import { useLocalSearchParams } from "expo-router";

import { TeamSettingsScreen } from "@/features/teams";

export default function Page() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <TeamSettingsScreen key={id} id={id} />;
}
