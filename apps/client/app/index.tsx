import { StyleSheet, Text, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

import { APP_NAME, platformLabel } from "@/lib/app-info";

/**
 * Placeholder home screen (T0.4.1). Real screens arrive with the app shell (T0.4.6); styling
 * moves to NativeWind tokens in T0.4.2.
 */
export default function Home() {
  return (
    <SafeAreaView style={styles.root}>
      <View style={styles.center}>
        <Text accessibilityRole="header" style={styles.title}>
          {APP_NAME}
        </Text>
        <Text style={styles.subtitle}>{platformLabel()}</Text>
      </View>
    </SafeAreaView>
  );
}

const styles = StyleSheet.create({
  root: { flex: 1, backgroundColor: "#ffffff" },
  center: { flex: 1, alignItems: "center", justifyContent: "center", gap: 8 },
  title: { fontSize: 28, fontWeight: "600", color: "#111827" },
  subtitle: { fontSize: 16, color: "#6b7280" },
});
