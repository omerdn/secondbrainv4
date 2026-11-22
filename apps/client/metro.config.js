const { execSync } = require("node:child_process");
const { existsSync, readdirSync, statSync } = require("node:fs");
const { join } = require("node:path");

const { getDefaultConfig } = require("expo/metro-config");
const { withNativeWind } = require("nativewind/metro");

/**
 * The native editor's WebView page (`src/lib/editor/native/editorHtml.js`, ADR-0019) is a build
 * output kept out of git. Build it before Metro starts when it is missing or older than its
 * sources, so a fresh clone or an EAS build never bundles a missing module.
 */
function ensureEditorBundle() {
  const output = join(__dirname, "src/lib/editor/native/editorHtml.js");
  const sources = [join(__dirname, "editor-web"), join(__dirname, "src/lib/editor/native")].flatMap(
    (dir) =>
      readdirSync(dir)
        .filter((name) => /\.(tsx?|html|mts)$/u.test(name) && !name.endsWith(".d.ts"))
        .map((name) => join(dir, name)),
  );
  const newest = Math.max(...sources.map((file) => statSync(file).mtimeMs));
  if (existsSync(output) && statSync(output).mtimeMs >= newest) return;
  execSync("pnpm editor:build", { cwd: __dirname, stdio: "inherit" });
}

ensureEditorBundle();

const config = getDefaultConfig(__dirname);

module.exports = withNativeWind(config, { input: "./global.css" });
