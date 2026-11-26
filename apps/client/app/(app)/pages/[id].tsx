import { useLocalSearchParams } from "expo-router";

import { PageScreen } from "@/features/notebooks";

export default function Page() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <PageScreen key={id} id={id} />;
}
