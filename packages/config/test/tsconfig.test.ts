import { readFileSync } from "node:fs";

import { describe, expect, it } from "vitest";

type TsConfig = { compilerOptions: Record<string, unknown> };

const base = JSON.parse(
  readFileSync(new URL("../tsconfig/base.json", import.meta.url), "utf8"),
) as TsConfig;

describe("@secondbrain/config/tsconfig/base.json", () => {
  it.each([
    "strict",
    "noUncheckedIndexedAccess",
    "exactOptionalPropertyTypes",
    "isolatedModules",
    "verbatimModuleSyntax",
  ])("enables %s", (option) => {
    expect(base.compilerOptions[option]).toBe(true);
  });
});
