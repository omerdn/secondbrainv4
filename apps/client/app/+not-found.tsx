import { Link, Stack } from "expo-router";
import { useTranslation } from "react-i18next";
import { Text, View } from "react-native";

/** Unknown routes (deep links, typed URLs on web). */
export default function NotFound() {
  const { t } = useTranslation();
  return (
    <>
      <Stack.Screen options={{ title: t("common.notFound.title") }} />
      <View className="flex-1 items-center justify-center gap-3 bg-background">
        <Text className="text-xl font-semibold text-foreground">{t("common.notFound.title")}</Text>
        <Link href="/" className="text-base text-primary">
          {t("common.notFound.home")}
        </Link>
      </View>
    </>
  );
}
