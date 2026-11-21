// Regenerates packages/api-types from the Rust DTOs (T0.2.7).
//   1. clears packages/api-types/src/generated (so removed types disappear),
//   2. runs only the ts-rs export tests (`export_bindings_*`; no database needed),
//   3. rebuilds packages/api-types/src/index.ts.
// Usage: pnpm api-types:generate   (requires cargo on PATH)
import { spawnSync } from "node:child_process";
import { rmSync } from "node:fs";
import { fileURLToPath } from "node:url";

const repo = fileURLToPath(new URL("..", import.meta.url));
const generated = `${repo}packages/api-types/src/generated`;

function run(command, args, cwd) {
  const result = spawnSync(command, args, { cwd, stdio: "inherit" });
  if (result.status !== 0) {
    console.error(`api-types: \`${command} ${args.join(" ")}\` failed`);
    process.exit(result.status ?? 1);
  }
}

rmSync(generated, { recursive: true, force: true });
run("cargo", ["test", "--workspace", "--quiet", "export_bindings"], `${repo}server`);
run("node", ["scripts/build-index.mjs"], `${repo}packages/api-types`);
