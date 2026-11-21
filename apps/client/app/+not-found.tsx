import { Link, Stack } from "expo-router";
import { Text, View } from "react-native";

/** Unknown routes (deep links, typed URLs on web). */
export default function NotFound() {
  return (
    <>
      <Stack.Screen options={{ title: "Not found" }} />
      <View className="flex-1 items-center justify-center gap-3 bg-background">
        <Text className="text-xl font-semibold text-foreground">Page not found</Text>
        <Link href="/" className="text-base text-primary">
          Go home
        </Link>
      </View>
    </>
  );
}
