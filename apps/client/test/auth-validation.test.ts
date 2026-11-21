import { ApiError } from "@secondbrain/api-client";

import {
  googleRedirectScheme,
  googleRedirectUri,
  providerFromError,
  webProviders,
  webStartUrl,
} from "@/features/auth/providers";
import { defaultTransport } from "@/features/auth/session";
import { t } from "@/features/auth/strings";
import {
  fromApiError,
  validateForgot,
  validateLogin,
  validateRegister,
  validateReset,
} from "@/features/auth/validation";

describe("validation (mirrors identity model.rs / password.rs)", () => {
  it("login needs an e-mail shape and any password", () => {
    expect(validateLogin({ email: "", password: "" })).toEqual({
      email: t.validation.required,
      password: t.validation.required,
    });
    expect(validateLogin({ email: "ali@", password: "x" })).toEqual({ email: t.validation.email });
    // Old short passwords can still sign in.
    expect(validateLogin({ email: " Ali@Example.com ", password: "short" })).toEqual({});
  });

  it("register checks name, e-mail and password length in characters", () => {
    expect(validateRegister({ displayName: "  ", email: "a@b.co", password: "12345678" })).toEqual({
      display_name: t.validation.required,
    });
    expect(
      validateRegister({ displayName: "ğ".repeat(101), email: "a@b.co", password: "1234567" }),
    ).toEqual({ display_name: t.validation.tooLong, password: t.validation.passwordShort });
    // 8 multi-byte characters are 8 characters, not 16 bytes.
    expect(validateRegister({ displayName: "Ali", email: "a@b.co", password: "şşşşşşşş" })).toEqual(
      {},
    );
    expect(
      validateRegister({ displayName: "Ali", email: "a@b.co", password: "x".repeat(129) }),
    ).toEqual({ password: t.validation.passwordLong });
    expect(
      validateRegister({
        displayName: "Ali",
        email: `${"a".repeat(250)}@b.co`,
        password: "12345678",
      }),
    ).toEqual({ email: t.validation.tooLong });
  });

  it("forgot needs an e-mail; reset needs matching new passwords", () => {
    expect(validateForgot({ email: "nope" })).toEqual({ email: t.validation.email });
    expect(validateReset({ password: "12345678", confirm: "12345679" })).toEqual({
      confirm: t.validation.mismatch,
    });
    // A too-short password is reported once, not also as a mismatch.
    expect(validateReset({ password: "123", confirm: "" })).toEqual({
      password: t.validation.passwordShort,
    });
    expect(validateReset({ password: "12345678", confirm: "12345678" })).toEqual({});
  });
});

describe("fromApiError", () => {
  it("maps server field errors to fields", () => {
    const error = new ApiError("validation_failed", "x", 422, {
      password: [{ field: "password", code: "too_short", message: "" }],
      email: [{ field: "email", code: "invalid", message: "" }],
      display_name: [{ field: "display_name", code: "too_long", message: "" }],
    });
    expect(fromApiError(error, "register")).toEqual({
      fields: {
        password: t.validation.passwordShort,
        email: t.validation.email,
        display_name: t.validation.tooLong,
      },
      form: null,
    });
    const unknownField = new ApiError("validation_failed", "x", 422, {
      device: [{ field: "device", code: "too_long", message: "" }],
    });
    expect(fromApiError(unknownField, "register").form).toBe(t.errors.generic);
  });

  it("maps codes to messages by context", () => {
    expect(fromApiError(new ApiError("unauthenticated", "", 401), "login").form).toBe(
      t.errors.badCredentials,
    );
    expect(fromApiError(new ApiError("conflict", "", 409), "register")).toEqual({
      fields: { email: t.errors.emailTaken },
      form: null,
    });
    expect(fromApiError(new ApiError("rate_limited", "", 429, {}, 30), "login").form).toBe(
      t.errors.rateLimited(30),
    );
    expect(t.errors.rateLimited(null)).not.toContain("null");
    expect(fromApiError(new ApiError("network", "", 0), "other").form).toBe(t.errors.network);
    expect(fromApiError(new ApiError("internal", "", 500), "other").form).toBe(t.errors.generic);
    expect(fromApiError(new Error("boom"), "other").form).toBe(t.errors.generic);
  });
});

describe("providers", () => {
  it("reads the provider of a failed web sign-in", () => {
    expect(providerFromError("google")).toBe("google");
    expect(providerFromError("apple")).toBe("apple");
    expect(providerFromError("evil")).toBeNull();
    expect(providerFromError(undefined)).toBeNull();
  });

  it("derives the Google iOS redirect from the client id", () => {
    const id = "123-abc.apps.googleusercontent.com";
    expect(googleRedirectScheme(id)).toBe("com.googleusercontent.apps.123-abc");
    expect(googleRedirectUri(id)).toBe("com.googleusercontent.apps.123-abc:/oauthredirect");
  });
});

describe("web", () => {
  it("offers only the providers the build enables", () => {
    expect(webProviders({ googleWeb: true, googleIosClientId: null, apple: true })).toEqual([
      "google",
      "apple",
    ]);
    expect(webProviders({ googleWeb: false, googleIosClientId: "ios", apple: false })).toEqual([]);
  });

  it("starts provider sign-in on the server", () => {
    expect(webStartUrl("http://localhost:8080/", "google")).toBe(
      "http://localhost:8080/auth/google/start",
    );
    expect(webStartUrl("https://api.example.com", "apple")).toBe(
      "https://api.example.com/auth/apple/start",
    );
  });

  it("uses the cookie transport only in browsers", () => {
    expect(defaultTransport("web")).toBe("cookie");
    expect(defaultTransport("ios")).toBe("bearer");
    expect(defaultTransport("android")).toBe("bearer");
  });
});
