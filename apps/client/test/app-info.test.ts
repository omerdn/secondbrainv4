import { readFileSync } from "node:fs";
import { join } from "node:path";

import appJson from "../app.json";
import desktopConfig from "../../desktop/src-tauri/tauri.conf.json";
import { themes } from "@secondbrain/ui";

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

// T0.7.6: brand assets exist at the sizes the platforms expect and use the primary colour.
describe("brand assets", () => {
  const pngSize = (file: string) => {
    const png = readFileSync(join(__dirname, "..", file));
    return { width: png.readUInt32BE(16), height: png.readUInt32BE(20) };
  };

  it("has a 1024 px app icon and square Android layers", () => {
    expect(pngSize(appJson.expo.icon)).toEqual({ width: 1024, height: 1024 });
    const adaptive = appJson.expo.android.adaptiveIcon;
    for (const file of [
      adaptive.foregroundImage,
      adaptive.backgroundImage,
      adaptive.monochromeImage,
    ]) {
      const { width, height } = pngSize(file);
      expect(width).toBe(height);
      expect(width).toBeGreaterThanOrEqual(432);
    }
  });

  it("uses the primary indigo behind the Android icon", () => {
    expect(appJson.expo.android.adaptiveIcon.backgroundColor.toLowerCase()).toBe(
      themes.light.primary.toLowerCase(),
    );
  });
});
