import { useLocalSearchParams } from "expo-router";

import { RoutineDetailScreen } from "@/features/routines";

export default function Page() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <RoutineDetailScreen key={id} id={id} />;
}
