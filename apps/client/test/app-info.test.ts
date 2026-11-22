import appJson from "../app.json";
import desktopConfig from "../../desktop/src-tauri/tauri.conf.json";
import { APP_NAME } from "@/lib/app-info";

// T0.7.4: the user-facing name is "Routune" on every platform (ADR-0021).
describe("app name", () => {
  it("is Routune in the Expo config and in the app", () => {
    expect(appJson.expo.name).toBe("Routune");
    expect(APP_NAME).toBe("Routune");
  });

  it("is Routune in the desktop shell", () => {
    expect(desktopConfig.productName).toBe("Routune");
    expect(desktopConfig.app.windows.map((w) => w.title)).toEqual(["Routune"]);
  });
});
