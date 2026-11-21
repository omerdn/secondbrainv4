import { ApiError } from "@secondbrain/api-client";

import { t } from "./strings";

/**
 * Client-side checks that mirror the server rules (identity `model.rs`, `password.rs`) so most
 * mistakes are shown before a request; the server stays the authority and its field errors are
 * shown the same way ({@link fromApiError}).
 */

export const MIN_PASSWORD_CHARS = 8;
export const MAX_PASSWORD_CHARS = 128;
export const MAX_EMAIL_CHARS = 254;
export const MAX_DISPLAY_NAME_CHARS = 100;

export type Field = "email" | "password" | "display_name" | "confirm";
export type FieldErrors = Partial<Record<Field, string>>;

/** Unicode scalar count, matching Rust's `chars().count()`. */
const chars = (value: string): number => [...value].length;

/** Deliberately loose: one `@` with text on both sides and a dot in the domain. */
const EMAIL_SHAPE = /^[^\s@]+@[^\s@]+\.[^\s@]+$/u;

function email(value: string): string | undefined {
  const trimmed = value.trim();
  if (trimmed.length === 0) return t.validation.required;
  if (chars(trimmed) > MAX_EMAIL_CHARS) return t.validation.tooLong;
  if (!EMAIL_SHAPE.test(trimmed)) return t.validation.email;
  return undefined;
}

function newPassword(value: string): string | undefined {
  if (value.length === 0) return t.validation.required;
  if (chars(value) < MIN_PASSWORD_CHARS) return t.validation.passwordShort;
  if (chars(value) > MAX_PASSWORD_CHARS) return t.validation.passwordLong;
  return undefined;
}

function compact(errors: FieldErrors): FieldErrors {
  return Object.fromEntries(Object.entries(errors).filter(([, message]) => message !== undefined));
}

export function validateLogin(input: { email: string; password: string }): FieldErrors {
  return compact({
    email: email(input.email),
    // Existing passwords are not re-checked against today's rules; only presence matters.
    password: input.password.length === 0 ? t.validation.required : undefined,
  });
}

export function validateRegister(input: {
  email: string;
  password: string;
  displayName: string;
}): FieldErrors {
  const name = input.displayName.trim();
  return compact({
    display_name:
      name.length === 0
        ? t.validation.required
        : chars(name) > MAX_DISPLAY_NAME_CHARS
          ? t.validation.tooLong
          : undefined,
    email: email(input.email),
    password: newPassword(input.password),
  });
}

export function validateForgot(input: { email: string }): FieldErrors {
  return compact({ email: email(input.email) });
}

export function validateReset(input: { password: string; confirm: string }): FieldErrors {
  const password = newPassword(input.password);
  return compact({
    password,
    confirm:
      password === undefined && input.password !== input.confirm
        ? t.validation.mismatch
        : undefined,
  });
}

export const hasErrors = (errors: FieldErrors): boolean => Object.keys(errors).length > 0;

const FIELD_CODE_MESSAGES: Record<string, string> = {
  required: t.validation.required,
  too_long: t.validation.tooLong,
  invalid: t.validation.invalid,
};

/** Server error → per-field messages plus one form-level message (or none). */
export function fromApiError(
  error: unknown,
  context: "login" | "register" | "other",
): { fields: FieldErrors; form: string | null } {
  if (!(error instanceof ApiError)) {
    return { fields: {}, form: t.errors.generic };
  }
  switch (error.code) {
    case "validation_failed": {
      const fields: FieldErrors = {};
      for (const field of ["email", "password", "display_name"] as const) {
        const first = error.fields[field]?.[0];
        if (first !== undefined) {
          fields[field] =
            field === "password" && first.code === "too_short"
              ? t.validation.passwordShort
              : field === "password" && first.code === "too_long"
                ? t.validation.passwordLong
                : field === "email" && first.code === "invalid"
                  ? t.validation.email
                  : (FIELD_CODE_MESSAGES[first.code] ?? t.validation.invalid);
        }
      }
      return { fields, form: hasErrors(fields) ? null : t.errors.generic };
    }
    case "unauthenticated":
      return { fields: {}, form: context === "login" ? t.errors.badCredentials : t.errors.generic };
    case "conflict":
      return {
        fields: context === "register" ? { email: t.errors.emailTaken } : {},
        form: context === "register" ? null : t.errors.generic,
      };
    case "rate_limited":
      return { fields: {}, form: t.errors.rateLimited(error.retryAfter) };
    case "network":
      return { fields: {}, form: t.errors.network };
    default:
      return { fields: {}, form: t.errors.generic };
  }
}
