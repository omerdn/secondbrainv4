import { ApiError } from "@secondbrain/api-client";

import { i18n } from "@/lib/i18n";

const t = i18n.t.bind(i18n);

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
  if (trimmed.length === 0) return t("auth.validation.required");
  if (chars(trimmed) > MAX_EMAIL_CHARS) return t("auth.validation.tooLong");
  if (!EMAIL_SHAPE.test(trimmed)) return t("auth.validation.email");
  return undefined;
}

function newPassword(value: string): string | undefined {
  if (value.length === 0) return t("auth.validation.required");
  if (chars(value) < MIN_PASSWORD_CHARS) return t("auth.validation.passwordShort");
  if (chars(value) > MAX_PASSWORD_CHARS) return t("auth.validation.passwordLong");
  return undefined;
}

function compact(errors: FieldErrors): FieldErrors {
  return Object.fromEntries(Object.entries(errors).filter(([, message]) => message !== undefined));
}

export function validateLogin(input: { email: string; password: string }): FieldErrors {
  return compact({
    email: email(input.email),
    // Existing passwords are not re-checked against today's rules; only presence matters.
    password: input.password.length === 0 ? t("auth.validation.required") : undefined,
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
        ? t("auth.validation.required")
        : chars(name) > MAX_DISPLAY_NAME_CHARS
          ? t("auth.validation.tooLong")
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
        ? t("auth.validation.mismatch")
        : undefined,
  });
}

export const hasErrors = (errors: FieldErrors): boolean => Object.keys(errors).length > 0;

/** Translated when used, so a language switch applies. */
const FIELD_CODE_KEYS = {
  required: "auth.validation.required",
  too_long: "auth.validation.tooLong",
  invalid: "auth.validation.invalid",
} as const;

function fieldCodeMessage(code: string): string {
  const key = FIELD_CODE_KEYS[code as keyof typeof FIELD_CODE_KEYS] as
    (typeof FIELD_CODE_KEYS)[keyof typeof FIELD_CODE_KEYS] | undefined;
  return t(key ?? "auth.validation.invalid");
}

/** Server error → per-field messages plus one form-level message (or none). */
export function fromApiError(
  error: unknown,
  context: "login" | "register" | "other",
): { fields: FieldErrors; form: string | null } {
  if (!(error instanceof ApiError)) {
    return { fields: {}, form: t("auth.errors.generic") };
  }
  switch (error.code) {
    case "validation_failed": {
      const fields: FieldErrors = {};
      for (const field of ["email", "password", "display_name"] as const) {
        const first = error.fields[field]?.[0];
        if (first !== undefined) {
          fields[field] =
            field === "password" && first.code === "too_short"
              ? t("auth.validation.passwordShort")
              : field === "password" && first.code === "too_long"
                ? t("auth.validation.passwordLong")
                : field === "email" && first.code === "invalid"
                  ? t("auth.validation.email")
                  : fieldCodeMessage(first.code);
        }
      }
      return { fields, form: hasErrors(fields) ? null : t("auth.errors.generic") };
    }
    case "unauthenticated":
      return {
        fields: {},
        form: context === "login" ? t("auth.errors.badCredentials") : t("auth.errors.generic"),
      };
    case "conflict":
      return {
        fields: context === "register" ? { email: t("auth.errors.emailTaken") } : {},
        form: context === "register" ? null : t("auth.errors.generic"),
      };
    case "rate_limited":
      return {
        fields: {},
        form:
          error.retryAfter === null
            ? t("auth.errors.rateLimited")
            : t("auth.errors.rateLimitedSeconds", { count: error.retryAfter }),
      };
    case "network":
      return { fields: {}, form: t("auth.errors.network") };
    default:
      return { fields: {}, form: t("auth.errors.generic") };
  }
}
