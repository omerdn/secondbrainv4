import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // React hook tests opt into a DOM with `// @vitest-environment happy-dom`.
    include: ["test/**/*.test.{ts,tsx}"],
  },
});
