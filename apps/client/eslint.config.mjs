import { baseConfig } from "@secondbrain/config/eslint";
import { defineConfig } from "eslint/config";
import globals from "globals";

export default defineConfig(baseConfig({ tsconfigRootDir: import.meta.dirname }), {
  files: ["test/**/*.{ts,tsx}"],
  languageOptions: { globals: { ...globals.jest } },
});
