import { ApiError } from "@secondbrain/api-client";

import {
  googleRedirectScheme,
  googleRedirectUri,
  providerFromError,
  webProviders,
  webStartUrl,
} from "@/features/auth/providers";
import { defaultTransport } from "@/features/auth/session";
import { i18n } from "@/lib/i18n";
import {
  fromApiError,
  validateForgot,
  validateLogin,
  validateRegister,
  validateReset,
} from "@/features/auth/validation";

const tr = i18n.getFixedT("tr");

describe("validation (mirrors identity model.rs / password.rs)", () => {
  it("login needs an e-mail shape and any password", () => {
    expect(validateLogin({ email: "", password: "" })).toEqual({
      email: tr("auth.validation.required"),
      password: tr("auth.validation.required"),
    });
    expect(validateLogin({ email: "ali@", password: "x" })).toEqual({
      email: tr("auth.validation.email"),
    });
    // Old short passwords can still sign in.
    expect(validateLogin({ email: " Ali@Example.com ", password: "short" })).toEqual({});
  });

  it("register checks name, e-mail and password length in characters", () => {
    expect(validateRegister({ displayName: "  ", email: "a@b.co", password: "12345678" })).toEqual({
      display_name: tr("auth.validation.required"),
    });
    expect(
      validateRegister({ displayName: "ğ".repeat(101), email: "a@b.co", password: "1234567" }),
    ).toEqual({
      display_name: tr("auth.validation.tooLong"),
      password: tr("auth.validation.passwordShort"),
    });
    // 8 multi-byte characters are 8 characters, not 16 bytes.
    expect(validateRegister({ displayName: "Ali", email: "a@b.co", password: "şşşşşşşş" })).toEqual(
      {},
    );
    expect(
      validateRegister({ displayName: "Ali", email: "a@b.co", password: "x".repeat(129) }),
    ).toEqual({ password: tr("auth.validation.passwordLong") });
    expect(
      validateRegister({
        displayName: "Ali",
        email: `${"a".repeat(250)}@b.co`,
        password: "12345678",
      }),
    ).toEqual({ email: tr("auth.validation.tooLong") });
  });

  it("forgot needs an e-mail; reset needs matching new passwords", () => {
    expect(validateForgot({ email: "nope" })).toEqual({ email: tr("auth.validation.email") });
    expect(validateReset({ password: "12345678", confirm: "12345679" })).toEqual({
      confirm: tr("auth.validation.mismatch"),
    });
    // A too-short password is reported once, not also as a mismatch.
    expect(validateReset({ password: "123", confirm: "" })).toEqual({
      password: tr("auth.validation.passwordShort"),
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
        password: tr("auth.validation.passwordShort"),
        email: tr("auth.validation.email"),
        display_name: tr("auth.validation.tooLong"),
      },
      form: null,
    });
    const unknownField = new ApiError("validation_failed", "x", 422, {
      device: [{ field: "device", code: "too_long", message: "" }],
    });
    expect(fromApiError(unknownField, "register").form).toBe(tr("auth.errors.generic"));
  });

  it("maps codes to messages by context", () => {
    expect(fromApiError(new ApiError("unauthenticated", "", 401), "login").form).toBe(
      tr("auth.errors.badCredentials"),
    );
    expect(fromApiError(new ApiError("conflict", "", 409), "register")).toEqual({
      fields: { email: tr("auth.errors.emailTaken") },
      form: null,
    });
    expect(fromApiError(new ApiError("rate_limited", "", 429, {}, 30), "login").form).toBe(
      tr("auth.errors.rateLimitedSeconds", { count: 30 }),
    );
    expect(tr("auth.errors.rateLimited")).not.toContain("{{");
    expect(fromApiError(new ApiError("network", "", 0), "other").form).toBe(
      tr("auth.errors.network"),
    );
    expect(fromApiError(new ApiError("internal", "", 500), "other").form).toBe(
      tr("auth.errors.generic"),
    );
    expect(fromApiError(new Error("boom"), "other").form).toBe(tr("auth.errors.generic"));
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
