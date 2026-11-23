import { useLocalSearchParams } from "expo-router";

import { ViewsScreen } from "@/features/views";

export default function Page() {
  const { view } = useLocalSearchParams<{ view?: string }>();
  // A different view starts from its own filter, not from the previous screen's edits.
  return <ViewsScreen key={view ?? "none"} viewId={view} />;
}
