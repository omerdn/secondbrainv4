import { Slot } from "expo-router";
import { renderRouter } from "expo-router/testing-library";

import type { TokenStore, Transport } from "@/features/auth/session";

import Home from "../../app/index";
import AuthLayout from "../../app/(auth)/_layout";
import ForgotPassword from "../../app/(auth)/forgot-password";
import Login from "../../app/(auth)/login";
import Register from "../../app/(auth)/register";
import ResetPassword from "../../app/reset-password";
import { type fakeServer, memoryTokenStore, Providers } from "./api";

/** The real routes under the test providers. */
export function renderApp(
  api: ReturnType<typeof fakeServer>["api"],
  initialUrl: string,
  {
    transport = "bearer",
    tokenStore = memoryTokenStore(),
  }: { transport?: Transport; tokenStore?: TokenStore } = {},
) {
  function Root() {
    return (
      <Providers api={api} transport={transport} tokenStore={tokenStore}>
        <Slot />
      </Providers>
    );
  }
  return renderRouter(
    {
      _layout: Root,
      index: Home,
      "(auth)/_layout": AuthLayout,
      "(auth)/login": Login,
      "(auth)/register": Register,
      "(auth)/forgot-password": ForgotPassword,
      "reset-password": ResetPassword,
    },
    { initialUrl },
  );
}
