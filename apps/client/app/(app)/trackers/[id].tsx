import { useLocalSearchParams } from "expo-router";

import { TrackerDetailScreen } from "@/features/trackers";

export default function Page() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <TrackerDetailScreen key={id} id={id} />;
}
