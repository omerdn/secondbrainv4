import { useLocalSearchParams } from "expo-router";

import { GoalDetailScreen } from "@/features/goals";

export default function Page() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <GoalDetailScreen key={id} id={id} />;
}
