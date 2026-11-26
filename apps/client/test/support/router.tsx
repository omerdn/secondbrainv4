import { Slot } from "expo-router";
import { renderRouter } from "expo-router/testing-library";

import type { TokenStore, Transport } from "@/features/auth/session";

import AppLayout from "../../app/(app)/_layout";
import Archive from "../../app/(app)/archive";
import Calendar from "../../app/(app)/calendar";
import Goals from "../../app/(app)/goals";
import Today from "../../app/(app)/index";
import ItemDetail from "../../app/(app)/items/[id]";
import More from "../../app/(app)/more";
import Notebooks from "../../app/(app)/notebooks";
import PageDetail from "../../app/(app)/pages/[id]";
import Routines from "../../app/(app)/routines";
import Session from "../../app/(app)/session";
import Settings from "../../app/(app)/settings";
import Tags from "../../app/(app)/tags";
import Tasks from "../../app/(app)/tasks";
import TaskDetail from "../../app/(app)/tasks/[id]";
import GoalDetail from "../../app/(app)/goals/[id]";
import RoutineDetail from "../../app/(app)/routines/[id]";
import Trackers from "../../app/(app)/trackers";
import TrackerDetail from "../../app/(app)/trackers/[id]";
import Trash from "../../app/(app)/trash";
import Views from "../../app/(app)/views";
import AuthLayout from "../../app/(auth)/_layout";
import ForgotPassword from "../../app/(auth)/forgot-password";
import Login from "../../app/(auth)/login";
import Register from "../../app/(auth)/register";
import ResetPassword from "../../app/reset-password";
import VerifyEmail from "../../app/verify-email";
import { type fakeServer, memoryTokenStore, Providers } from "./api";

/** The real routes under the test providers. */
export function renderApp(
  api: ReturnType<typeof fakeServer>["api"],
  initialUrl: string,
  {
    transport = "bearer",
    tokenStore = memoryTokenStore(),
    devices,
  }: { transport?: Transport; tokenStore?: TokenStore; devices?: () => string[] } = {},
) {
  function Root() {
    return (
      <Providers api={api} transport={transport} tokenStore={tokenStore} devices={devices}>
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
      "(app)/tasks/[id]": TaskDetail,
      "(app)/routines": Routines,
      "(app)/goals": Goals,
      "(app)/goals/[id]": GoalDetail,
      "(app)/routines/[id]": RoutineDetail,
      "(app)/trackers": Trackers,
      "(app)/trackers/[id]": TrackerDetail,
      "(app)/notebooks": Notebooks,
      "(app)/pages/[id]": PageDetail,
      "(app)/calendar": Calendar,
      "(app)/tags": Tags,
      "(app)/views": Views,
      "(app)/archive": Archive,
      "(app)/trash": Trash,
      "(app)/session": Session,
      "(app)/settings": Settings,
      "(app)/more": More,
      "(app)/items/[id]": ItemDetail,
      "(auth)/_layout": AuthLayout,
      "(auth)/login": Login,
      "(auth)/register": Register,
      "(auth)/forgot-password": ForgotPassword,
      "reset-password": ResetPassword,
      "verify-email": VerifyEmail,
    },
    { initialUrl },
  );
}
