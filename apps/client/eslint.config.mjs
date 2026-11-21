import { join } from "node:path";

import { baseConfig, featureBoundaries } from "@secondbrain/config/eslint";
import { defineConfig } from "eslint/config";
import globals from "globals";

export default defineConfig(
  baseConfig({ tsconfigRootDir: import.meta.dirname }),
  // features/x may only use features/y's public index; src/lib imports no feature
  // (ARCHITECTURE §4.6). Tests are exempt: they exercise internals on purpose.
  featureBoundaries({ srcDir: join(import.meta.dirname, "src") }),
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
