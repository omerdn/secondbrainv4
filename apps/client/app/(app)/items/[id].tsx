import { useLocalSearchParams } from "expo-router";

import { ItemDetailScreen } from "@/features/items";

export default function Page() {
  const { id } = useLocalSearchParams<{ id: string }>();
  return <ItemDetailScreen key={id} id={id} />;
}
