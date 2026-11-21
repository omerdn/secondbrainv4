import path from "node:path";

import { Linter } from "eslint";
import { describe, expect, it } from "vitest";

import { featureBoundaries, featureOf } from "../src/feature-boundaries.js";

const app = "/repo/apps/client";
const src = path.join(app, "src");

function lint(file: string, code: string): string[] {
  const linter = new Linter({ cwd: app });
  const messages = linter.verify(
    code,
    [
      { languageOptions: { ecmaVersion: "latest", sourceType: "module" } },
      featureBoundaries({ srcDir: src, files: ["**/*.ts"] }),
    ],
    { filename: path.join(app, file) },
  );
  return messages.map((m) => {
    if (m.fatal === true) throw new Error(m.message);
    return m.message;
  });
}

describe("featureOf", () => {
  const features = path.join(src, "features");
  it("finds the feature and the path inside it", () => {
    expect(featureOf(features, path.join(features, "auth", "session.tsx"))).toEqual({
      name: "auth",
      rest: ["session.tsx"],
    });
    expect(featureOf(features, path.join(features, "auth"))).toEqual({ name: "auth", rest: [] });
    expect(featureOf(features, path.join(features, "modules", "budget", "x.ts"))).toEqual({
      name: "modules/budget",
      rest: ["x.ts"],
    });
    expect(featureOf(features, path.join(src, "lib", "api.ts"))).toBeNull();
    expect(featureOf(features, features)).toBeNull();
  });
});

describe("secondbrain/feature-boundaries", () => {
  it("allows a feature's public entry from anywhere", () => {
    for (const spec of ["@/features/auth", "@/features/auth/index", "@/features/auth/index.ts"]) {
      expect(lint("src/features/shell/a.ts", `import { x } from "${spec}";`)).toEqual([]);
      expect(lint("app/(app)/page.ts", `import { x } from "${spec}";`)).toEqual([]);
    }
  });

  it("allows any file of the same feature, by alias or relative path", () => {
    expect(lint("src/features/auth/a.ts", 'import { x } from "@/features/auth/session";')).toEqual(
      [],
    );
    expect(lint("src/features/auth/deep/a.ts", 'import { x } from "../session";')).toEqual([]);
  });

  it("allows packages and shared code", () => {
    expect(
      lint(
        "src/features/auth/a.ts",
        'import { x } from "@secondbrain/ui";\nimport { y } from "@/lib/api";\nimport z from "react";',
      ),
    ).toEqual([]);
  });

  it("rejects another feature's internal files", () => {
    const cases: [string, string][] = [
      ["src/features/shell/a.ts", 'import { x } from "@/features/auth/session";'],
      ["src/features/shell/a.ts", 'import { x } from "../auth/session";'],
      ["src/features/shell/deep/a.ts", 'import { x } from "../../auth/deep/file";'],
      ["src/features/shell/a.ts", 'export { x } from "@/features/auth/session";'],
      ["src/features/shell/a.ts", 'export * from "@/features/auth/session";'],
      ["src/features/shell/a.ts", 'const m = import("@/features/auth/session");'],
      ["app/(app)/page.ts", 'import { x } from "@/features/auth/session";'],
      ["src/features/modules/budget/a.ts", 'import { x } from "@/features/modules/series/x";'],
    ];
    for (const [file, code] of cases) {
      const messages = lint(file, code);
      expect(messages, `${file}: ${code}`).toHaveLength(1);
      expect(messages[0]).toContain("iç dosyası import edilemez");
    }
  });

  it("rejects any feature import from src/lib", () => {
    const [message] = lint("src/lib/api.ts", 'import { x } from "@/features/auth";');
    expect(message).toContain("`src/lib` paylaşılan koddur");
  });
});
