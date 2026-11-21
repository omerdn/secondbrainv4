import { type ComponentRef, type Ref, useId } from "react";
import { TextInput, type TextInputProps, View } from "react-native";

import { cx } from "../cx";
import { Text } from "./Text";

export interface InputProps extends Omit<TextInputProps, "style"> {
  label: string;
  /** Help text under the field; replaced by `error` when that is set. */
  hint?: string;
  /** Validation message (e.g. a server field error). Marks the field invalid. */
  error?: string | null;
  className?: string;
  /** React 19 passes `ref` as a prop. */
  ref?: Ref<ComponentRef<typeof TextInput>>;
}

/** Labelled text field with hint and error text. */
export function Input({ label, hint, error, className, editable, ref, ...rest }: InputProps) {
  const id = useId();
  const invalid = typeof error === "string" && error.length > 0;
  const message = invalid ? error : hint;
  return (
    <View className={cx("gap-1.5", className)}>
      <Text variant="label" nativeID={`${id}-label`}>
        {label}
      </Text>
      <TextInput
        ref={ref}
        accessibilityLabel={label}
        accessibilityLabelledBy={`${id}-label`}
        accessibilityHint={message}
        aria-invalid={invalid}
        editable={editable}
        placeholderTextColor="rgb(107 114 128)"
        className={cx(
          "min-h-10 rounded-md border bg-surface-raised px-3 py-2 text-base text-foreground",
          invalid ? "border-danger" : "border-border",
          editable === false && "opacity-50",
        )}
        {...rest}
      />
      {message !== undefined ? (
        <Text
          variant="caption"
          tone={invalid ? "danger" : "muted"}
          accessibilityLiveRegion={invalid ? "polite" : "none"}
        >
          {message}
        </Text>
      ) : null}
    </View>
  );
}
