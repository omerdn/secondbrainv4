import { Button, Input, Sheet, Text } from "@secondbrain/ui";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { View } from "react-native";

import { type TitleError, titleError } from "./model";

/** Asks for a name: a new notebook or page, or a new name for one. What was typed stays if
 * saving fails. */
export function NameSheet({
  visible,
  title,
  submitLabel,
  initial = "",
  saving,
  failed,
  onClose,
  onSubmit,
}: {
  visible: boolean;
  title: string;
  submitLabel: string;
  initial?: string;
  saving: boolean;
  failed: boolean;
  onClose: () => void;
  onSubmit: (name: string) => void;
}) {
  const { t } = useTranslation();
  const [name, setName] = useState(initial);
  const [error, setError] = useState<TitleError | null>(null);
  useEffect(() => {
    if (visible) {
      setName(initial);
      setError(null);
    }
  }, [visible, initial]);
  const submit = () => {
    const problem = titleError(name);
    setError(problem);
    if (problem === null) onSubmit(name.trim());
  };

  return (
    <Sheet
      visible={visible}
      onClose={onClose}
      title={title}
      closeLabel={t("tags.close")}
      footer={<Button title={submitLabel} loading={saving} disabled={saving} onPress={submit} />}
    >
      <View className="gap-3">
        <Input
          label={t("notebooks.name")}
          value={name}
          error={error === null ? undefined : t(`notebooks.errors.${error}`)}
          onChangeText={setName}
          onSubmitEditing={submit}
        />
        {failed ? (
          <Text tone="danger" accessibilityRole="alert">
            {t("notebooks.saveFailed")}
          </Text>
        ) : null}
      </View>
    </Sheet>
  );
}
