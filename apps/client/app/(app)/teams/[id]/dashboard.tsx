import { useLocalSearchParams } from "expo-router";

import { TeamDashboardScreen } from "@/features/teams";

export default function Page() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <TeamDashboardScreen key={id} id={id} />;
}
