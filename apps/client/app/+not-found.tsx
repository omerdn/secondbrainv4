import { Link, Stack } from "expo-router";
import { StyleSheet, Text, View } from "react-native";

/** Unknown routes (deep links, typed URLs on web). */
export default function NotFound() {
  return (
    <>
      <Stack.Screen options={{ title: "Not found" }} />
      <View style={styles.center}>
        <Text style={styles.title}>Page not found</Text>
        <Link href="/" style={styles.link}>
          Go home
        </Link>
      </View>
    </>
  );
}

const styles = StyleSheet.create({
  center: { flex: 1, alignItems: "center", justifyContent: "center", gap: 12 },
  title: { fontSize: 20, fontWeight: "600" },
  link: { fontSize: 16, color: "#2563eb" },
});
