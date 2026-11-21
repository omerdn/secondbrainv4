import { baseConfig } from "@secondbrain/config/eslint";
import { defineConfig } from "eslint/config";
import globals from "globals";

export default defineConfig(
  baseConfig({ tsconfigRootDir: import.meta.dirname }),
  {
    files: ["babel.config.cjs"],
    languageOptions: { sourceType: "commonjs", globals: { ...globals.node } },
    rules: {
      "@typescript-eslint/no-unsafe-call": "off",
      "@typescript-eslint/no-unsafe-member-access": "off",
    },
  },
  {
    files: ["test/**/*.{ts,tsx}"],
    languageOptions: { globals: { ...globals.jest } },
  },
);
