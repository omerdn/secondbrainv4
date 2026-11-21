import {
  Avatar,
  Button,
  Card,
  EmptyState,
  Input,
  Sheet,
  TagChip,
  Text,
  useToast,
} from "@secondbrain/ui";
import { Redirect } from "expo-router";
import { useState } from "react";
import { ScrollView, View } from "react-native";
import { SafeAreaView } from "react-native-safe-area-context";

/**
 * Development-only gallery of the base components (T0.4.3), used to check them on web and
 * mobile. Production builds redirect to the home screen.
 */
export default function ComponentGallery() {
  if (!__DEV__) {
    return <Redirect href="/" />;
  }
  return <Gallery />;
}

function Section({ title, children }: { title: string; children: React.ReactNode }) {
  return (
    <View className="gap-3">
      <Text variant="heading">{title}</Text>
      {children}
    </View>
  );
}

function Gallery() {
  const toast = useToast();
  const [sheetOpen, setSheetOpen] = useState(false);
  const [email, setEmail] = useState("");
  const [selected, setSelected] = useState(false);

  return (
    <SafeAreaView className="flex-1 bg-background">
      <ScrollView contentContainerClassName="gap-8 p-4">
        <Text variant="title">Components</Text>

        <Section title="Text">
          <Text>Body text</Text>
          <Text tone="muted">Muted text</Text>
          <Text variant="caption" tone="danger">
            Danger caption
          </Text>
        </Section>

        <Section title="Button">
          <View className="flex-row flex-wrap gap-2">
            <Button title="Primary" onPress={() => toast.show({ message: "Primary pressed" })} />
            <Button title="Secondary" variant="secondary" />
            <Button title="Ghost" variant="ghost" />
            <Button title="Danger" variant="danger" />
            <Button title="Loading" loading />
            <Button title="Small" size="sm" />
          </View>
        </Section>

        <Section title="Input">
          <Input
            label="E-mail"
            hint="Used for sign-in"
            value={email}
            onChangeText={setEmail}
            autoCapitalize="none"
            keyboardType="email-address"
          />
          <Input label="Display name" error="must not be blank" />
        </Section>

        <Section title="Card">
          <Card>
            <Text>Static card</Text>
          </Card>
          <Card onPress={() => toast.show({ message: "Card pressed" })} accessibilityLabel="Open">
            <Text>Pressable card</Text>
          </Card>
        </Section>

        <Section title="Avatar">
          <View className="flex-row items-center gap-3">
            <Avatar name="Ada Lovelace" size="sm" />
            <Avatar name="ilker şahin" />
            <Avatar name="Broken Image" url="https://invalid.example/none.png" size="lg" />
          </View>
        </Section>

        <Section title="TagChip">
          <View className="flex-row flex-wrap gap-2">
            <TagChip label="health" />
            <TagChip
              label="selectable"
              selected={selected}
              onPress={() => setSelected((s) => !s)}
            />
            <TagChip label="sprint" teamColor="#1e3a8a" />
            <TagChip label="design" teamColor="#fde047" onRemove={() => undefined} />
          </View>
        </Section>

        <Section title="Sheet & Toast">
          <View className="flex-row flex-wrap gap-2">
            <Button title="Open sheet" variant="secondary" onPress={() => setSheetOpen(true)} />
            <Button
              title="Error toast"
              variant="secondary"
              onPress={() => toast.show({ message: "Something went wrong", tone: "danger" })}
            />
            <Button
              title="Success toast"
              variant="secondary"
              onPress={() => toast.show({ message: "Saved", tone: "success" })}
            />
          </View>
        </Section>

        <Section title="EmptyState">
          <Card>
            <EmptyState
              title="No tasks yet"
              description="Tasks you create or are assigned to show up here."
              action={{ title: "Create task", onPress: () => toast.show({ message: "Create" }) }}
            />
          </Card>
        </Section>
      </ScrollView>

      <Sheet
        visible={sheetOpen}
        onClose={() => setSheetOpen(false)}
        title="Filters"
        footer={
          <>
            <Button title="Cancel" variant="ghost" onPress={() => setSheetOpen(false)} />
            <Button title="Apply" onPress={() => setSheetOpen(false)} />
          </>
        }
      >
        <Text tone="muted">Sheet content goes here.</Text>
      </Sheet>
    </SafeAreaView>
  );
}
