import { Slot } from "expo-router";
import { renderRouter } from "expo-router/testing-library";

import type { TokenStore, Transport } from "@/features/auth/session";

import AppLayout from "../../app/(app)/_layout";
import Calendar from "../../app/(app)/calendar";
import Goals from "../../app/(app)/goals";
import Today from "../../app/(app)/index";
import More from "../../app/(app)/more";
import Notebooks from "../../app/(app)/notebooks";
import Routines from "../../app/(app)/routines";
import Settings from "../../app/(app)/settings";
import Tasks from "../../app/(app)/tasks";
import Trackers from "../../app/(app)/trackers";
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
      "(app)/_layout": AppLayout,
      "(app)/index": Today,
      "(app)/tasks": Tasks,
      "(app)/routines": Routines,
      "(app)/goals": Goals,
      "(app)/trackers": Trackers,
      "(app)/notebooks": Notebooks,
      "(app)/calendar": Calendar,
      "(app)/settings": Settings,
      "(app)/more": More,
      "(auth)/_layout": AuthLayout,
      "(auth)/login": Login,
      "(auth)/register": Register,
      "(auth)/forgot-password": ForgotPassword,
      "reset-password": ResetPassword,
    },
    { initialUrl },
  );
}
