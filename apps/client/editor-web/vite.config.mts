import { createRequire } from "node:module";
import { dirname, join } from "node:path";

import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { viteSingleFile } from "vite-plugin-singlefile";

const require = createRequire(import.meta.url);
const tentap = dirname(require.resolve("@10play/tentap-editor/package.json"));

/**
 * One self-contained HTML file for the WebView's `customSource` (10tap advanced setup).
 *
 * `@10play/tentap-editor/web` ships with its own copy of TipTap inlined; TipTap's Collaboration
 * extension then sees a different `@tiptap/core` and fails (`editor.utils` is undefined). Building
 * TenTap's web utilities from source with one deduplicated TipTap, Yjs and React avoids that.
 */
export default defineConfig({
  root: import.meta.dirname,
  plugins: [react(), viteSingleFile()],
  resolve: {
    alias: [
      {
        find: /^@10play\/tentap-editor\/web$/u,
        replacement: join(tentap, "src/webEditorUtils/index.ts"),
      },
    ],
    dedupe: ["@tiptap/core", "@tiptap/pm", "yjs", "react", "react-dom"],
  },
  build: { outDir: "dist", emptyOutDir: true },
});
