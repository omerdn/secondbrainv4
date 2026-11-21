import { baseConfig } from "@secondbrain/config/eslint";
import { defineConfig } from "eslint/config";
import globals from "globals";

export default defineConfig(
  baseConfig({ tsconfigRootDir: import.meta.dirname }),
  {
    // CommonJS config files read by Babel and Metro.
    files: ["babel.config.js", "metro.config.js"],
    languageOptions: { sourceType: "commonjs", globals: { ...globals.node } },
    rules: {
      "@typescript-eslint/no-require-imports": "off",
      "@typescript-eslint/no-unsafe-assignment": "off",
      "@typescript-eslint/no-unsafe-call": "off",
      "@typescript-eslint/no-unsafe-member-access": "off",
    },
  },
  {
    files: ["test/**/*.{ts,tsx}"],
    languageOptions: { globals: { ...globals.jest } },
  },
);
