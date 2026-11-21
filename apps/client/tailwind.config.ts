import uiPreset from "@secondbrain/ui/tailwind-preset";
import type { Config } from "tailwindcss";
// eslint-disable-next-line @typescript-eslint/no-require-imports
const nativewindPreset = require("nativewind/preset") as NonNullable<Config["presets"]>[number];

/** Classes come only from the `@secondbrain/ui` tokens (ADR-0017). */
const config: Config = {
  content: ["./app/**/*.{ts,tsx}", "./src/**/*.{ts,tsx}", "../../packages/ui/src/**/*.{ts,tsx}"],
  presets: [nativewindPreset, uiPreset],
};

export default config;
