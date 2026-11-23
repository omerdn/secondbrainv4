import { useLocalSearchParams } from "expo-router";

import { TaskDetailScreen } from "@/features/tasks";

export default function Page() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <TaskDetailScreen key={id} id={id} />;
}
